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
}

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
        found.push(Found {
            raw: chars[start..end + suffix_len.min(chars.len() - end)].iter().collect(),
            value,
            tolerance: 0.5 * 10f64.powi(-(decimals as i32)) * multiplier,
            percent,
            plain_int: decimals == 0 && multiplier == 1.0 && !percent && !had_separator,
            dated,
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
    let target = found.value.abs();
    let slack = found.tolerance * 1.001 + 1e-9;
    known.iter().any(|fact| {
        let fact = fact.abs();
        (target - fact).abs() <= slack
            || (target - fact * 100.0).abs() <= slack
            || (found.percent && (target / 100.0 - fact).abs() <= slack / 100.0)
    })
}

/// Numbers in `text` that appear in neither `facts` nor the user's `question`. Years, dates and small
/// whole numbers (10 or less) are ignored, so the check only fires on figures that look computed.
pub fn untraced(text: &str, facts: &Value, question: &str) -> Vec<String> {
    let mut known = Vec::new();
    collect(facts, &mut known);
    known.extend(scan(question).into_iter().map(|found| found.value));
    let mut missing: Vec<String> = Vec::new();
    for found in scan(text) {
        if found.dated || (found.plain_int && (found.value <= 10.0 || (1900.0..=2100.0).contains(&found.value))) {
            continue;
        }
        if !traced(&found, &known) && !missing.contains(&found.raw) {
            missing.push(found.raw.trim().to_owned());
        }
    }
    missing
}
