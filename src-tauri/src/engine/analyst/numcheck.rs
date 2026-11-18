//! The model narrates; code owns the arithmetic. This finds numbers in narrated text that cannot be traced
//! to the FACTS the model was given, allowing for rounding, percent, thousands separators and K/M/B suffixes.

use serde_json::Value;

struct Found {
    raw: String,
    value: f64,
    /// Half of the last printed digit, in the value's own units: how far rounding can move it.
    tolerance: f64,
    percent: bool,
    plain_int: bool,
    dated: bool,
    /// Printed as a year: after a month name or a word like "in"/"since"/"year".
    year_context: bool,
    /// Preceded by a word claiming an increase ("up 5%", "grew 5%").
    rise: bool,
}

const RISE_WORDS: [&str; 5] = ["up", "increased", "rose", "grew", "gained"];

const YEAR_WORDS: [&str; 24] = [
    "in", "since", "year", "fy", "by", "during", "until", "through", "of", "january", "february", "march", "april", "may", "june",
    "july", "august", "september", "october", "november", "december", "sept", "from", "to",
];

fn scan(text: &str) -> Vec<Found> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let start = index;
        if !chars[index].is_ascii_digit() || (index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_')) {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < chars.len() && (chars[end].is_ascii_digit() || ((chars[end] == ',' || chars[end] == '.') && chars.get(end + 1).is_some_and(char::is_ascii_digit))) {
            end += 1;
        }
        index = end;
        let body: String = chars[start..end].iter().collect();
        let had_separator = body.contains(',');
        let cleaned = body.replace(',', "");
        let Ok(mut value) = cleaned.parse::<f64>() else { continue };
        let decimals = cleaned.split_once('.').map_or(0, |(_, fraction)| fraction.len());
        let rest: String = chars[end..].iter().take(9).collect::<String>().to_lowercase();
        let mut multiplier = 1.0;
        let mut percent = false;
        let mut suffix_len = 0;
        let trimmed = rest.trim_start();
        let gap = rest.len() - trimmed.len();
        for (word, scale) in [("billion", 1e9), ("million", 1e6), ("thousand", 1e3), ("bn", 1e9), ("b", 1e9), ("m", 1e6), ("k", 1e3)] {
            if trimmed.starts_with(word) && !trimmed[word.len()..].chars().next().is_some_and(char::is_alphanumeric) && (gap == 0 || word.len() > 1) {
                multiplier = scale;
                suffix_len = gap + word.len();
                break;
            }
        }
        if multiplier == 1.0 && rest.starts_with('%') {
            percent = true;
            suffix_len = 1;
        }
        index = end + suffix_len;
        value *= multiplier;
        let before = if start > 0 { chars[start - 1] } else { ' ' };
        let after = chars.get(end).copied().unwrap_or(' ');
        let dated = (matches!(before, '-' | '/') && start > 1 && chars[start - 2].is_ascii_digit())
            || (matches!(after, '-' | '/' | ':') && chars.get(end + 1).is_some_and(char::is_ascii_digit));
        let previous_word: String = chars[..start].iter().rev().skip_while(|c| c.is_whitespace()).take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect::<String>().to_lowercase();
        let year_context = YEAR_WORDS.contains(&previous_word.as_str());
        found.push(Found {
            raw: chars[start..end + suffix_len.min(chars.len() - end)].iter().collect(),
            value,
            tolerance: 0.5 * 10f64.powi(-(decimals as i32)) * multiplier,
            percent,
            plain_int: decimals == 0 && multiplier == 1.0 && !percent && !had_separator,
            dated,
            year_context,
            rise: RISE_WORDS.contains(&previous_word.as_str()),
        });
    }
    found
}

fn collect(facts: &Value, out: &mut Vec<f64>) {
    match facts {
        Value::Number(number) => out.extend(number.as_f64()),
        Value::String(text) => out.extend(scan(text).into_iter().map(|found| found.value)),
        Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect(item, out)),
        _ => {}
    }
}

fn traced(found: &Found, known: &[f64]) -> bool {
    matching(found, known).next().is_some()
}

/// The known values (signed) that the narrated number matches by magnitude.
fn matching<'a>(found: &'a Found, known: &'a [f64]) -> impl Iterator<Item = f64> + 'a {
    let target = found.value.abs();
    let slack = found.tolerance * 1.001 + 1e-9;
    known.iter().copied().filter(move |signed| {
        let fact = signed.abs();
        (target - fact).abs() <= slack
            || (found.percent && (target - fact * 100.0).abs() <= slack)
            || (found.percent && (target / 100.0 - fact).abs() <= slack / 100.0)
    })
}

/// Numbers in `text` that appear in neither `facts` nor the user's `question`. Years, dates and small
/// whole numbers (10 or less) are ignored (years only after a year-like word), so the check only fires on figures that look computed.
pub fn untraced(text: &str, facts: &Value, question: &str) -> Vec<String> {
    let mut known = Vec::new();
    collect(facts, &mut known);
    known.extend(scan(question).into_iter().map(|found| found.value));
    let mut missing: Vec<String> = Vec::new();
    for found in scan(text) {
        if found.dated || (found.plain_int && (found.value <= 10.0 || (found.year_context && (1900.0..=2100.0).contains(&found.value)))) {
            continue;
        }
        // shortcut: only "up X" against negative-only data; "down X" vs positive is a common positive-magnitude phrasing, too noisy.
        let wrong_sign = found.rise && { let mut m = matching(&found, &known).peekable(); m.peek().is_some() && m.all(|v| v < 0.0) };
        let label = if wrong_sign { format!("{} (data is negative)", found.raw.trim()) } else { found.raw.trim().to_owned() };
        if (wrong_sign || !traced(&found, &known)) && !missing.contains(&label) {
            missing.push(label);
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn years_skip_only_in_year_context() {
        assert!(untraced("Sales peaked in 2023.", &json!({}), "").is_empty());
        assert_eq!(untraced("There were 2023 orders.", &json!({}), ""), vec!["2023"]);
    }

    #[test]
    fn times_100_only_for_percents() {
        assert!(untraced("Growth was 12%.", &json!({"g": 0.12}), "").is_empty());
        assert_eq!(untraced("Total was 1200.", &json!({"g": 12}), ""), vec!["1200"]);
    }

    #[test]
    fn up_claim_against_negative_data_is_flagged() {
        assert_eq!(untraced("Revenue was up 5%.", &json!({"g": -0.05}), ""), vec!["5% (data is negative)"]);
        assert!(untraced("Revenue was up 5%.", &json!({"g": 0.05}), "").is_empty());
        assert!(untraced("Revenue was down 5%.", &json!({"g": -0.05}), "").is_empty());
    }

    #[test]
    fn rounding_and_suffixes_trace() {
        assert!(untraced("About 1.2M units.", &json!({"n": 1_204_000}), "").is_empty());
    }
}
