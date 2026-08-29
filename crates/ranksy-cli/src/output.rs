use tabled::builder::Builder;
use tabled::settings::Style;

#[derive(Clone, Copy)]
pub enum Format {
    Table,
    Json,
}

pub struct Column {
    pub header: &'static str,
    pub key: &'static str,
}

#[allow(dead_code)] // part of the planned output API; wired when color styles land
pub fn color_enabled() -> bool {
    use is_terminal::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn rows(value: &serde_json::Value) -> Vec<serde_json::Value> {
    match value {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => match o.get("data") {
            Some(serde_json::Value::Array(a)) => a.clone(),
            _ => vec![value.clone()],
        },
        _ => vec![value.clone()],
    }
}

fn cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Ordered union of object keys across rows. Used when a command supplies no
/// hand-picked columns (the analytics-parity endpoints), so the table still
/// reflects whatever fields the API actually returned.
fn auto_keys(rows: &[serde_json::Value]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for row in rows {
        if let serde_json::Value::Object(o) = row {
            for k in o.keys() {
                if !keys.iter().any(|seen| seen == k) {
                    keys.push(k.clone());
                }
            }
        }
    }
    keys
}

pub fn render(value: &serde_json::Value, format: Format, columns: &[Column]) -> String {
    match format {
        Format::Json => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        Format::Table => {
            let data = rows(value);
            let mut builder = Builder::default();
            if columns.is_empty() {
                let keys = auto_keys(&data);
                if keys.is_empty() {
                    return serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
                }
                builder.push_record(keys.iter().cloned());
                for row in &data {
                    builder.push_record(keys.iter().map(|k| cell(row.get(k).unwrap_or(&serde_json::Value::Null))));
                }
            } else {
                builder.push_record(columns.iter().map(|c| c.header.to_string()));
                for row in &data {
                    builder.push_record(columns.iter().map(|c| cell(row.get(c.key).unwrap_or(&serde_json::Value::Null))));
                }
            }
            builder.build().with(Style::rounded()).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cols() -> Vec<Column> {
        vec![
            Column { header: "Keyword", key: "keyword" },
            Column { header: "Position", key: "position" },
        ]
    }

    #[test]
    fn json_format_is_pretty_passthrough() {
        let v = json!({"a": 1});
        let out = render(&v, Format::Json, &[]);
        assert!(out.contains("\"a\": 1"));
    }

    #[test]
    fn table_lists_rows_from_array() {
        let v = json!([
            {"keyword": "shopify seo", "position": 3},
            {"keyword": "app store",  "position": 12}
        ]);
        let out = render(&v, Format::Table, &cols());
        assert!(out.contains("Keyword"));
        assert!(out.contains("shopify seo"));
        assert!(out.contains("12"));
    }

    #[test]
    fn table_unwraps_data_envelope() {
        let v = json!({"data": [{"keyword": "x", "position": 1}]});
        let out = render(&v, Format::Table, &cols());
        assert!(out.contains("x"));
    }
}
