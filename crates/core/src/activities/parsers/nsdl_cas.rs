//! NSDL e-CAS (National Securities Depository Limited) parser.

use regex::Regex;

/// Parse NSDL e-CAS text into tabular headers and rows.
///
/// Output columns matching `system_nsdl_cas`:
/// - `Transaction Date`
/// - `Particulars`
/// - `Security Name`
/// - `ISIN`
/// - `Quantity`
/// - `Rate`
/// - `Amount`
/// - `Client ID`
pub fn parse_nsdl_cas(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = vec![
        "Transaction Date".to_string(),
        "Particulars".to_string(),
        "Security Name".to_string(),
        "ISIN".to_string(),
        "Quantity".to_string(),
        "Rate".to_string(),
        "Amount".to_string(),
        "Client ID".to_string(),
    ];

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_client_id = String::new();
    let mut current_security = String::new();
    let mut current_isin = String::new();

    let client_id_regex =
        Regex::new(r"(?i)Client\s*ID[:\s]+([0-9A-Za-z]+)").expect("hard-coded regex is valid");
    let isin_regex = Regex::new(r"\b(IN[A-Z0-9]{10})\b").expect("hard-coded regex is valid");
    let date_regex =
        Regex::new(r"^\b(\d{1,2}[-/][A-Za-z]{3}[-/]\d{2,4}|\d{1,2}[-/]\d{1,2}[-/]\d{2,4})\b")
            .expect("hard-coded regex is valid");

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 1. Detect Client ID
        if let Some(caps) = client_id_regex.captures(trimmed) {
            current_client_id = caps.get(1).map_or("", |m| m.as_str()).to_string();
        }

        // 2. Detect ISIN and Security
        if let Some(caps) = isin_regex.captures(trimmed) {
            current_isin = caps.get(1).map_or("", |m| m.as_str()).to_string();
            let remainder = trimmed
                .replace(&current_isin, "")
                .replace("ISIN:", "")
                .replace("ISIN", "");
            let s = remainder.trim();
            if s.len() > 3 {
                current_security = s.to_string();
            }
        }

        // 3. Detect Transaction Row
        if let Some(date_match) = date_regex.find(trimmed) {
            let date_str = date_match.as_str().to_string();
            let remainder = trimmed[date_match.end()..].trim();

            let tokens: Vec<&str> = remainder.split_whitespace().collect();
            if tokens.len() >= 2 {
                let mut num_indices = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    let cleaned = t.replace([',', '(', ')'], "");
                    if cleaned.parse::<f64>().is_ok() {
                        num_indices.push(i);
                    }
                }

                if !num_indices.is_empty() {
                    let first_num_idx = num_indices[0];
                    let particulars = tokens[..first_num_idx].join(" ");
                    let mut qty = String::new();
                    let mut rate = String::new();
                    let mut amount = String::new();

                    if num_indices.len() == 1 {
                        qty = tokens[num_indices[0]].replace(',', "");
                    } else if num_indices.len() == 2 {
                        qty = tokens[num_indices[0]].replace(',', "");
                        rate = tokens[num_indices[1]].replace(',', "");
                    } else if num_indices.len() >= 3 {
                        qty = tokens[num_indices[0]].replace(',', "");
                        rate = tokens[num_indices[1]].replace(',', "");
                        amount = tokens[num_indices[2]].replace(',', "");
                    }

                    if !particulars.is_empty() && !qty.is_empty() {
                        rows.push(vec![
                            date_str,
                            particulars,
                            current_security.clone(),
                            current_isin.clone(),
                            qty,
                            rate,
                            amount,
                            current_client_id.clone(),
                        ]);
                    }
                }
            }
        }
    }

    (headers, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nsdl_sample() {
        let sample = r#"
Client ID: 10023456
ISIN: INE002A01018 RELIANCE INDUSTRIES LTD
10-May-2024 BUY 10 2850.50 28505.00
"#;
        let (headers, rows) = parse_nsdl_cas(sample);
        assert_eq!(headers.len(), 8);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], "10-May-2024");
        assert_eq!(rows[0][1], "BUY");
        assert_eq!(rows[0][2], "RELIANCE INDUSTRIES LTD");
        assert_eq!(rows[0][3], "INE002A01018");
        assert_eq!(rows[0][4], "10");
        assert_eq!(rows[0][7], "10023456");
    }
}
