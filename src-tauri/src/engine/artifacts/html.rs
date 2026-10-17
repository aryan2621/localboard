//! Self-contained HTML for live datasites, static exports and print reports.

use serde_json::{json, Value};

use super::{esc, json_script, BRAND_HEX, BRAND_NAME};
use crate::storage::artifacts::{DatasitePage, DatasiteRecord, ReportRecord};

pub enum SiteMode<'a> {
    Live,
    Static,
    Linked { origin: &'a str },
}

pub fn datasite_page(site: &DatasiteRecord, page: &DatasitePage, mode: SiteMode<'_>, brand: &str, brand_name: &str) -> String {
    let nav = site
        .pages
        .iter()
        .map(|item| {
            let href = match mode {
                SiteMode::Live => {
                    if item.slug == "overview" {
                        format!("/d/{}", esc(&site.id))
                    } else {
                        format!("/d/{}/{}", esc(&site.id), esc(&item.slug))
                    }
                }
                SiteMode::Static | SiteMode::Linked { .. } => format!("#{}", esc(&item.slug)),
            };
            let current = if item.id == page.id { " aria-current=\"page\"" } else { "" };
            format!("<a href=\"{href}\"{current}>{}</a>", esc(&item.title))
        })
        .collect::<Vec<_>>()
        .join("");

    let mut data = serde_json::Map::new();
    let mut charts = String::new();
    for chart in &page.charts {
        if let Some(result) = &chart.result {
            data.insert(chart.id.clone(), serde_json::to_value(result).unwrap_or(Value::Null));
        }
        let spec = serde_json::to_value(&chart.chart).unwrap_or(Value::Null);
        charts.push_str(&format!(
            "<section class=\"tile\"><h2>{}</h2><div class=\"chart\" data-id=\"{}\" data-spec=\"{}\"></div></section>",
            esc(&chart.title),
            esc(&chart.id),
            esc(&spec.to_string())
        ));
    }

    let fetch = match mode {
        SiteMode::Live => format!("const BASE=''; const SITE='{}';", esc(&site.id)),
        SiteMode::Static => "const BASE=null; const SITE='';".to_owned(),
        SiteMode::Linked { origin } => format!("const BASE='{}'; const SITE='{}';", esc(origin), esc(&site.id)),
    };

    wrap_named(
        &site.name,
        brand,
        brand_name,
        &format!(
            "<header class=\"bar\"><strong>{}</strong><span>{}</span></header>\
             <nav>{nav}</nav>\
             <main><h1>{}</h1><p class=\"lead\">{}</p>{charts}</main>\
             <script>window.__LB_DATA__={};{fetch}{CHART_JS}</script>",
            esc(brand_name),
            esc(&page.title),
            esc(&page.title),
            esc(&page.narrative),
            json_script(&Value::Object(data))
        ),
    )
}

pub fn datasite_bundle(site: &DatasiteRecord, mode: SiteMode<'_>, brand: &str, brand_name: &str) -> String {
    let mut sections = String::new();
    for page in &site.pages {
        let mut charts = String::new();
        for chart in &page.charts {
            let spec = serde_json::to_value(&chart.chart).unwrap_or(Value::Null);
            charts.push_str(&format!(
                "<section class=\"tile\"><h2>{}</h2><div class=\"chart\" data-id=\"{}\" data-spec=\"{}\"></div></section>",
                esc(&chart.title),
                esc(&chart.id),
                esc(&spec.to_string())
            ));
        }
        sections.push_str(&format!(
            "<article id=\"{}\"><h1>{}</h1><p class=\"lead\">{}</p>{charts}</article>",
            esc(&page.slug),
            esc(&page.title),
            esc(&page.narrative)
        ));
    }
    let nav = site
        .pages
        .iter()
        .map(|page| format!("<a href=\"#{}\">{}</a>", esc(&page.slug), esc(&page.title)))
        .collect::<Vec<_>>()
        .join("");
    let mut data = serde_json::Map::new();
    for page in &site.pages {
        for chart in &page.charts {
            if let Some(result) = &chart.result {
                data.insert(chart.id.clone(), serde_json::to_value(result).unwrap_or(Value::Null));
            }
        }
    }
    let fetch = match mode {
        SiteMode::Live => format!("const BASE=''; const SITE='{}';", esc(&site.id)),
        SiteMode::Static => "const BASE=null; const SITE='';".to_owned(),
        SiteMode::Linked { origin } => format!("const BASE='{}'; const SITE='{}';", esc(origin), esc(&site.id)),
    };
    wrap_named(
        &site.name,
        brand,
        brand_name,
        &format!(
            "<header class=\"bar\"><strong>{}</strong><span>{}</span></header>\
             <nav>{nav}</nav>\
             <main>{sections}</main>\
             <script>window.__LB_DATA__={};{fetch}{CHART_JS}</script>",
            esc(brand_name),
            esc(&site.name),
            json_script(&Value::Object(data))
        ),
    )
}

pub fn report_html(report: &ReportRecord) -> String {
    let brand = report.theme.get("brand").and_then(Value::as_str).unwrap_or(BRAND_HEX);
    let brand_name = report.theme.get("name").and_then(Value::as_str).unwrap_or(BRAND_NAME);
    let mut body = String::new();
    let mut appendix = String::new();
    let mut data = serde_json::Map::new();
    for block in &report.blocks {
        if block.kind == "narrative" {
            body.push_str(&format!("<section><h2>{}</h2><p>{}</p></section>", esc(&block.title), esc(&block.body)));
            continue;
        }
        if let Some(result) = &block.result {
            data.insert(block.id.clone(), serde_json::to_value(result).unwrap_or(Value::Null));
        }
        if let Some(sql) = &block.sql {
            appendix.push_str(&format!("<h3>{}</h3><pre>{}</pre>", esc(&block.title), esc(sql)));
        }
        let spec = block.chart.as_ref().map(|chart| serde_json::to_value(chart).unwrap_or(Value::Null)).unwrap_or(json!({ "kind": "table", "queryId": block.id, "x": null, "y": [] }));
        body.push_str(&format!(
            "<section><h2>{}</h2><p>{}</p><div class=\"chart\" data-id=\"{}\" data-spec=\"{}\"></div></section>",
            esc(&block.title),
            esc(&block.body),
            esc(&block.id),
            esc(&spec.to_string())
        ));
    }
    if !appendix.is_empty() {
        body.push_str(&format!("<section class=\"appendix\"><h2>SQL appendix</h2>{appendix}</section>"));
    }
    wrap_brand(
        &report.name,
        brand,
        &format!(
            "<header class=\"bar\"><strong>{}</strong><span>{}</span></header>\
             <main><h1>{}</h1>{body}</main>\
             <script>window.__LB_DATA__={};const BASE=null;const SITE='';{CHART_JS}</script>",
            esc(brand_name),
            esc(&report.name),
            esc(&report.name),
            json_script(&Value::Object(data))
        ),
    )
}

fn wrap_named(title: &str, brand: &str, _brand_name: &str, body: &str) -> String {
    wrap_brand(title, brand, body)
}

fn wrap_brand(title: &str, brand: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\
         <title>{}</title><style>{css}</style></head><body>{body}</body></html>",
        esc(title),
        css = CSS.replace("BRAND", brand)
    )
}

const CSS: &str = "\
:root{--brand:BRAND;--ink:#1c1917;--muted:#78716c;--line:#e7e5e4;--paper:#fffaf5}\
*{box-sizing:border-box}body{margin:0;font:15px/1.5 ui-sans-serif,system-ui,sans-serif;color:var(--ink);background:var(--paper)}\
.bar{display:flex;justify-content:space-between;align-items:center;padding:14px 28px;background:var(--brand);color:#fff}\
.bar strong{font-family:ui-serif,Georgia,serif;font-size:18px}\
nav{display:flex;gap:12px;flex-wrap:wrap;padding:12px 28px;border-bottom:1px solid var(--line);background:#fff}\
nav a{color:var(--ink);text-decoration:none;font-size:13px}nav a[aria-current]{color:var(--brand);font-weight:600}\
main{max-width:960px;margin:0 auto;padding:28px}h1{font:32px/1.2 ui-serif,Georgia,serif;margin:0 0 12px}\
h2{font:18px/1.3 ui-serif,Georgia,serif;margin:28px 0 8px}.lead{color:var(--muted);max-width:40rem}\
.tile{margin:24px 0;padding:16px;border:1px solid var(--line);border-radius:12px;background:#fff}\
.chart{min-height:220px}table{width:100%;border-collapse:collapse;font-size:13px}\
th,td{border-bottom:1px solid var(--line);text-align:left;padding:6px 8px}th{color:var(--muted);font-weight:600}\
.kpi{font:40px/1.1 ui-serif,Georgia,serif;color:var(--brand)}pre{white-space:pre-wrap;background:#fff;border:1px solid var(--line);padding:12px;border-radius:8px;font-size:12px}\
.appendix{margin-top:48px}@media print{nav{display:none}.bar{print-color-adjust:exact;-webkit-print-color-adjust:exact}}\
";

const CHART_JS: &str = r#"
function val(v){return v==null?'':String(v)}
function col(result,name){return result.columns.findIndex(c=>c.name===name)}
function nums(result,name){const i=col(result,name);return result.rows.map(r=>Number(r[i])).filter(n=>!isNaN(n))}
async function resultOf(id){
  if(BASE===null) return window.__LB_DATA__[id];
  const url=(BASE||'')+'/d/'+SITE+'/data/'+id;
  const res=await fetch(url);
  if(!res.ok) throw new Error('Refresh failed');
  return res.json();
}
function table(el,result){
  const h=result.columns.map(c=>'<th>'+esc(c.name)+'</th>').join('');
  const rows=result.rows.slice(0,50).map(r=>'<tr>'+r.map(c=>'<td>'+esc(val(c))+'</td>').join('')+'</tr>').join('');
  el.innerHTML='<table><thead><tr>'+h+'</tr></thead><tbody>'+rows+'</tbody></table>';
}
function kpi(el,result,spec){
  const name=(spec.y&&spec.y[0])||(result.columns.find(c=>c.kind==='number')||{}).name;
  const i=col(result,name); const v=result.rows[0]?result.rows[0][i]:'';
  el.innerHTML='<div class="kpi">'+esc(val(v))+'</div><p>'+esc(name||'')+'</p>';
}
function bars(el,result,spec){
  const x=spec.x||(result.columns[0]||{}).name;
  const y=(spec.y&&spec.y[0])||(result.columns.find(c=>c.kind==='number')||{}).name;
  const xi=col(result,x), yi=col(result,y);
  const vals=result.rows.map(r=>Number(r[yi])).filter(n=>!isNaN(n));
  const max=Math.max(1,...vals.map(Math.abs));
  el.innerHTML='<svg viewBox="0 0 400 180" width="100%" height="180">'+
    result.rows.slice(0,24).map((r,i)=>{
      const n=Number(r[yi])||0; const h=Math.max(2,Math.round(Math.abs(n)/max*140));
      return '<rect x="'+(16+i*16)+'" y="'+(160-h)+'" width="12" height="'+h+'" fill="var(--brand)"/>';
    }).join('')+'</svg>';
}
function pie(el,result,spec){
  const x=spec.x||(result.columns[0]||{}).name; const y=(spec.y&&spec.y[0])||'';
  const xi=col(result,x), yi=col(result,y);
  const parts=result.rows.slice(0,6).map(r=>({l:val(r[xi]),v:Math.abs(Number(r[yi])||0)}));
  const sum=parts.reduce((a,p)=>a+p.v,0)||1; let a=0;
  const colors=['#C96240','#1c1917','#a8a29e','#78716c','#d6d3d1','#44403c'];
  el.innerHTML='<svg viewBox="0 0 200 200" width="200" height="200">'+parts.map((p,i)=>{
    const s=a; a+=p.v/sum*Math.PI*2; const large=p.v/sum>0.5?1:0;
    const x1=100+80*Math.cos(s-Math.PI/2), y1=100+80*Math.sin(s-Math.PI/2);
    const x2=100+80*Math.cos(a-Math.PI/2), y2=100+80*Math.sin(a-Math.PI/2);
    return '<path d="M100,100 L'+x1+','+y1+' A80,80 0 '+large+' 1 '+x2+','+y2+' Z" fill="'+colors[i%colors.length]+'"/>';
  }).join('')+'</svg>';
}
function draw(el,spec,result){
  if(!result||!result.columns){el.textContent='No data';return;}
  const kind=spec.kind||'table';
  if(kind==='kpi') return kpi(el,result,spec);
  if(kind==='pie'||kind==='funnel') return pie(el,result,spec);
  if(kind==='table'||kind==='heatmap') return table(el,result);
  return bars(el,result,spec);
}
function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')}
document.querySelectorAll('.chart').forEach(async el=>{
  try{ draw(el, JSON.parse(el.dataset.spec||'{}'), await resultOf(el.dataset.id)); }
  catch(e){ el.textContent=e.message||'Could not load'; }
});
"#;
