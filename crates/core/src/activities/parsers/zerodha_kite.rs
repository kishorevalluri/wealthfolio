//! Zerodha Kite tradebook / statement parser.
//!
//! Handles standard Zerodha Kite tradebook and holdings exports.
//! Fields typically include:
//! `trade_date`, `trade_type`, `symbol`, `isin`, `quantity`, `price`, `trade_id`, `segment`

/// Header definitions matching Zerodha Kite exports.
pub fn zerodha_tradebook_headers() -> Vec<String> {
    vec![
        "trade_date".to_string(),
        "trade_type".to_string(),
        "symbol".to_string(),
        "isin".to_string(),
        "quantity".to_string(),
        "price".to_string(),
        "trade_id".to_string(),
        "segment".to_string(),
    ]
}

/// Parse Zerodha Kite text or lines if extracted from PDF / plain text.
pub fn parse_zerodha_tradebook(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let headers = zerodha_tradebook_headers();
    let mut rows: Vec<Vec<String>> = Vec::new();

    let date_regex = regex::Regex::new(r"^\b(\d{4}-\d{2}-\d{2}|\d{2}-\d{2}-\d{4})\b")
        .expect("hard-coded regex is valid");

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check if starts with a date
        if let Some(date_match) = date_regex.find(trimmed) {
            let date = date_match.as_str().to_string();
            let tokens: Vec<&str> = trimmed[date_match.end()..].split_whitespace().collect();

            // e.g. BUY INFY INE009A01021 10 1500.00 TR123456 EQ
            if tokens.len() >= 4 {
                let trade_type = tokens[0].to_uppercase();
                if trade_type == "BUY" || trade_type == "SELL" {
                    let symbol = tokens[1].to_string();
                    let mut isin = String::new();
                    let mut qty = String::new();
                    let mut price = String::new();
                    let mut trade_id = String::new();
                    let mut segment = "EQ".to_string();

                    let mut next_idx = 2;
                    if tokens.len() > next_idx
                        && tokens[next_idx].starts_with("IN")
                        && tokens[next_idx].len() == 12
                    {
                        isin = tokens[next_idx].to_string();
                        next_idx += 1;
                    }

                    if tokens.len() > next_idx {
                        qty = tokens[next_idx].replace(',', "");
                        next_idx += 1;
                    }

                    if tokens.len() > next_idx {
                        price = tokens[next_idx].replace(',', "");
                        next_idx += 1;
                    }

                    if tokens.len() > next_idx {
                        trade_id = tokens[next_idx].to_string();
                        next_idx += 1;
                    }

                    if tokens.len() > next_idx {
                        segment = tokens[next_idx].to_string();
                    }

                    if !qty.is_empty() && !price.is_empty() {
                        rows.push(vec![
                            date, trade_type, symbol, isin, qty, price, trade_id, segment,
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
    fn test_parse_zerodha_sample() {
        let sample = "2024-05-10 BUY INFY INE009A01021 10 1450.50 8912345 EQ\n2024-05-12 SELL TCS INE467B01029 5 3850.00 8912346 EQ";
        let (headers, rows) = parse_zerodha_tradebook(sample);
        assert_eq!(headers.len(), 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "2024-05-10");
        assert_eq!(rows[0][1], "BUY");
        assert_eq!(rows[0][2], "INFY");
        assert_eq!(rows[0][3], "INE009A01021");
    }
}
