//! Fallback generic table extractor for unknown PDFs / statements.

use regex::Regex;

/// Fallback generic table extractor for arbitrary tabular text.
pub fn parse_generic_pdf_table(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let date_regex = Regex::new(r"\b(\d{1,2}[-/][A-Za-z]{3}[-/]\d{2,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4}|\d{4}[-/]\d{1,2}[-/]\d{1,2})\b").unwrap();

    let headers = vec![
        "Date".to_string(),
        "Description".to_string(),
        "Quantity".to_string(),
        "Price".to_string(),
        "Amount".to_string(),
    ];

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(m) = date_regex.find(line) {
            let date = m.as_str().to_string();
            let remainder = line[m.end()..].trim();
            let tokens: Vec<&str> = remainder.split_whitespace().collect();

            if tokens.len() >= 2 {
                let mut num_indices = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    let cleaned = t.replace(',', "");
                    if cleaned.parse::<f64>().is_ok() {
                        num_indices.push(i);
                    }
                }

                if !num_indices.is_empty() {
                    let first_num = num_indices[0];
                    let desc = tokens[..first_num].join(" ");
                    let qty = tokens.get(first_num).unwrap_or(&"").to_string();
                    let price = num_indices.get(1).and_then(|&idx| tokens.get(idx)).unwrap_or(&"").to_string();
                    let amount = num_indices.get(2).and_then(|&idx| tokens.get(idx)).unwrap_or(&"").to_string();

                    rows.push(vec![date, desc, qty, price, amount]);
                }
            }
        }
    }

    (headers, rows)
}
