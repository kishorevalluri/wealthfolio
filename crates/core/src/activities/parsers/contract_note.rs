//! Broker contract note parser (Zerodha, Upstox, Dhan).
//!
//! A contract note lists one trading day's trades and the charges levied on
//! them. Brokers have changed the layout several times, and the text a PDF
//! yields keeps the reading order but not the columns, so the parser reads the
//! note as one stream of words and relies on what every layout shares:
//!
//! - each trade starts with an order number and time, then a trade number and
//!   time, followed by the security, the side (B/S or BUY/SELL), the quantity
//!   and a run of rates that ends with the trade's value, which is the
//!   quantity times one of those rates;
//! - the note states the net amount payable or receivable, and the securities
//!   transaction tax (STT).
//!
//! The charges are the difference between the net amount and the trades'
//! value. STT becomes each trade's tax and the rest (brokerage, exchange and
//! SEBI fees, stamp duty, GST) its fee, both shared out by trade value. Fills
//! of one security on one side are combined into one row, priced at their
//! average.

use std::collections::HashMap;

use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

/// Columns the parser writes, in order.
pub fn contract_note_headers() -> Vec<String> {
    [
        "Date",
        "Broker",
        "Contract Note",
        "Side",
        "Symbol",
        "ISIN",
        "Name",
        "Quantity",
        "Price",
        "Value",
        "Fee",
        "Tax",
        "Currency",
        "Comment",
    ]
    .iter()
    .map(|h| h.to_string())
    .collect()
}

/// Whether extracted PDF text is a broker's contract note.
pub fn is_contract_note(text: &str) -> bool {
    let lower = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    lower.contains("contract note") && lower.contains("trade date")
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractNoteTrade {
    pub time: String,
    pub side: Side,
    pub symbol: String,
    pub isin: Option<String>,
    pub name: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub value: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
}

/// One contract note, read.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractNote {
    pub broker: String,
    pub number: String,
    pub trade_date: Option<NaiveDate>,
    pub trades: Vec<ContractNoteTrade>,
    /// Securities transaction tax for the whole note.
    pub stt: Decimal,
    /// Every other charge for the whole note.
    pub fees: Decimal,
    /// Problems a reader should check by hand.
    pub warnings: Vec<String>,
}

/// A number token: `1,094.75`, `(957.80)`, `-0.46`. Brackets and DR/CR mark a
/// side, not a sign, so they read as magnitudes; a bare minus is kept.
fn number_of(token: &str) -> Option<(Decimal, bool)> {
    let t = token.trim_end_matches(',');
    let bracketed = t.starts_with('(') && t.ends_with(')') && t.len() > 2;
    let inner = if bracketed { &t[1..t.len() - 1] } else { t };
    let cleaned = inner.replace(',', "");
    if cleaned.is_empty()
        || !cleaned
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
        || !cleaned.chars().any(|c| c.is_ascii_digit())
    {
        return None;
    }
    let value = Decimal::from_str(&cleaned).ok()?;
    Some((if bracketed { value.abs() } else { value }, bracketed))
}

fn is_number(token: &str) -> bool {
    number_of(token).is_some()
}

/// Tokens that sit inside a run of numbers without ending it.
fn is_filler(token: &str) -> bool {
    matches!(
        token,
        "DR" | "CR"
            | "Dr"
            | "Cr"
            | "₹"
            | "(₹)"
            | "("
            | ")"
            | "(Rs.)"
            | "(Rs)"
            | "Rs."
            | "*"
            | "**"
            | "***"
            | "#"
    )
}

/// The numbers right after `start`, skipping filler tokens. DR/CR make the
/// number before them a magnitude.
fn number_run(tokens: &[&str], start: usize) -> Vec<Decimal> {
    collect_run(tokens, start, false)
}

/// Like `number_run`, for amounts in rupees and paise: a whole number ends
/// the run, as it is a footnote mark ("(3539.66) 1 Applicable only ...").
fn amount_run(tokens: &[&str], start: usize) -> Vec<Decimal> {
    collect_run(tokens, start, true)
}

fn collect_run(tokens: &[&str], start: usize, amounts_only: bool) -> Vec<Decimal> {
    let mut run = Vec::new();
    let mut i = start;
    while i < tokens.len() {
        let token = tokens[i];
        if amounts_only && number_of(token).is_some() && !token.contains('.') {
            break;
        }
        if let Some((value, _)) = number_of(token) {
            run.push(value);
        } else if matches!(token, "DR" | "CR" | "Dr" | "Cr") {
            if let Some(last) = run.last_mut() {
                *last = last.abs();
            }
        } else if !is_filler(token) {
            break;
        }
        i += 1;
    }
    run
}

fn isin_regex() -> Regex {
    Regex::new(r"\bIN[A-Z0-9]{9}[0-9]\b").expect("hard-coded regex is valid")
}

fn is_time(token: &str) -> bool {
    let b = token.as_bytes();
    b.len() == 8
        && b[2] == b':'
        && b[5] == b':'
        && [0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_digit())
}

fn is_order_number(token: &str) -> bool {
    token.len() >= 5 && token.chars().all(|c| c.is_ascii_digit())
}

/// Starts of trade records: order number, time, trade number, time.
fn trade_starts(tokens: &[&str]) -> Vec<usize> {
    (0..tokens.len().saturating_sub(3))
        .filter(|&i| {
            is_order_number(tokens[i])
                && is_time(tokens[i + 1])
                && tokens[i + 2].chars().all(|c| c.is_ascii_digit())
                && !tokens[i + 2].is_empty()
                && is_time(tokens[i + 3])
        })
        .collect()
}

fn side_of(token: &str) -> Option<Side> {
    match token {
        "B" | "BUY" | "Buy" => Some(Side::Buy),
        "S" | "SELL" | "Sell" => Some(Side::Sell),
        _ => None,
    }
}

/// Words between the side and the quantity.
fn is_exchange(token: &str) -> bool {
    matches!(token, "NSE" | "BSE" | "NSE-M" | "BSE-M")
}

/// Reads one trade from its tokens (from the order number to the next trade).
fn read_trade(tokens: &[&str], isin_re: &Regex) -> Option<ContractNoteTrade> {
    let time = tokens.get(3)?.to_string();
    // The side is the first B/S word followed, past an exchange, by a number.
    let mut side_at = None;
    for i in 4..tokens.len() {
        if let Some(side) = side_of(tokens[i]) {
            let mut j = i + 1;
            while j < tokens.len() && is_exchange(tokens[j]) {
                j += 1;
            }
            if j < tokens.len() && is_number(tokens[j]) {
                side_at = Some((i, side, j));
                break;
            }
        }
    }
    let (side_index, side, qty_index) = side_at?;
    let quantity = number_of(tokens[qty_index])?.0.abs();
    if quantity.is_zero() {
        return None;
    }
    // Rates follow the quantity; Dhan puts a "D" (delivery) between them.
    let mut rates_start = qty_index + 1;
    if tokens.get(rates_start).is_some_and(|t| *t == "D") {
        rates_start += 1;
    }
    let run = number_run(tokens, rates_start);
    let (price, value) = price_and_value(quantity, &run)?;

    let description = tokens[4..side_index].join(" ");
    // Dhan names the ISIN after the trade rather than in its description.
    let isin = isin_re
        .find(&description)
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            let after = tokens[qty_index..].join(" ");
            isin_re.find(&after).map(|m| m.as_str().to_string())
        });
    let (symbol, name) = symbol_and_name(&description, isin.as_deref());
    Some(ContractNoteTrade {
        time,
        side,
        symbol,
        isin,
        name,
        quantity,
        price,
        value,
    })
}

/// The trade's value and price per unit: the last number in the run that is
/// the quantity times an earlier positive number in it.
fn price_and_value(quantity: Decimal, run: &[Decimal]) -> Option<(Decimal, Decimal)> {
    for (t_index, total) in run.iter().enumerate().rev() {
        let total = total.abs();
        if total.is_zero() {
            continue;
        }
        let tolerance = (total * Decimal::new(5, 4)).max(Decimal::new(5, 2));
        let found = run[..t_index]
            .iter()
            .rev()
            .map(|r| r.abs())
            .filter(|r| !r.is_zero())
            .find(|r| (quantity * *r - total).abs() <= tolerance);
        if let Some(_rate) = found {
            return Some((total / quantity, total));
        }
    }
    None
}

/// The symbol and name from a trade's description, in each layout:
/// `HDFC-EQ/INE001A01036`, `CADILAHC / INE010B01027`, `BIRLASOFT
/// [INE836A01035]`, `(DIVISLAB NSE) Divi s Laboratories Limited`,
/// `APOLLOHOSP-APOLLO HOSPITALS ENTERPRISE LT`, `SIEMENS LTD`.
fn symbol_and_name(description: &str, isin: Option<&str>) -> (String, String) {
    let mut text = description.to_string();
    if let Some(isin) = isin {
        text = text.replace(isin, " ");
    }
    let text = text
        .replace(['[', ']'], " ")
        .replace(" / ", " ")
        .trim_end_matches('/')
        .trim()
        .to_string();
    // Drop a segment heading such as EQ-CASH or NCL-NSE-Equity-M.
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| !w.contains("CASH") && !w.starts_with("NCL") && *w != "Equity")
        .collect();
    let text = words.join(" ");

    let bracket = Regex::new(r"^\(([A-Z0-9&_.-]+)\s+(?:NSE|BSE)\)\s*(.*)$").expect("valid");
    if let Some(c) = bracket.captures(&text) {
        return (c[1].to_string(), c[2].trim().to_string());
    }
    let first = words.first().copied().unwrap_or_default();
    let first = first.trim_end_matches('/');
    // Zerodha: SYMBOL-EQ / SYMBOL-BE; Dhan: SYMBOL-COMPANY NAME.
    if let Some((symbol, rest)) = first.split_once('-') {
        let series = ["EQ", "BE", "BZ", "SM", "ST", "E1", "IV", "RR", "GS"];
        if series.contains(&rest) {
            return (symbol.to_string(), symbol.to_string());
        }
        if !symbol.is_empty()
            && symbol
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '&')
        {
            let name = std::iter::once(rest)
                .chain(words.iter().skip(1).copied())
                .collect::<Vec<_>>()
                .join(" ");
            return (symbol.to_string(), name);
        }
    }
    if words.len() == 1 {
        return (first.to_string(), first.to_string());
    }
    // Upstox and old Dhan notes give only a name.
    (String::new(), text)
}

/// Symbol → ISIN pairs that a note lists outside its trades: Dhan's
/// `Symbol :KIMS ISIN : INE967H01025` and summary rows `INE437A01024
/// APOLLOHOSP 1 ...`.
fn isins_by_symbol(stream: &str, isin_re: &Regex) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let labelled = Regex::new(r"Symbol\s*:\s*([A-Z0-9&_.-]+)\s+ISIN\s*:\s*(IN[A-Z0-9]{9}[0-9])")
        .expect("valid");
    for c in labelled.captures_iter(stream) {
        map.insert(c[1].to_string(), c[2].to_string());
    }
    let tokens: Vec<&str> = stream.split(' ').collect();
    for (i, token) in tokens.iter().enumerate() {
        if isin_re.is_match(token) && token.len() == 12 {
            if let Some(next) = tokens.get(i + 1) {
                let next = next.trim_end_matches(',');
                if !next.is_empty()
                    && next.chars().all(|c| {
                        c.is_ascii_uppercase() || c.is_ascii_digit() || c == '&' || c == '-'
                    })
                    && next.chars().any(|c| c.is_ascii_uppercase())
                {
                    map.entry(next.to_string())
                        .or_insert_with(|| token.to_string());
                }
            }
        }
    }
    map
}

fn detect_broker(lower: &str) -> &'static str {
    if lower.contains("zerodha") {
        "Zerodha"
    } else if lower.contains("upstox") || lower.contains("rksv") {
        "Upstox"
    } else if lower.contains("dhan") || lower.contains("moneylicious") {
        "Dhan"
    } else {
        "Broker"
    }
}

fn trade_date_of(stream: &str) -> Option<NaiveDate> {
    let re = Regex::new(
        r"(?i)trade\s*date\s*:?\s*(\d{4}-\d{2}-\d{2}|\d{2}[/-]\d{2}[/-]\d{4}|[A-Za-z]{3}\s+\d{1,2}\s+\d{4})",
    )
    .expect("valid");
    let raw = re.captures(stream)?.get(1)?.as_str().to_string();
    for format in ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y", "%b %d %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&raw, format) {
            return Some(date);
        }
    }
    None
}

fn note_number_of(stream: &str) -> String {
    let re =
        Regex::new(r"(?i)contract\s*note\s*no\.?\s*:?\s*([A-Z]*-?\d[A-Z0-9/-]*)").expect("valid");
    if let Some(c) = re.captures(stream) {
        return c[1].to_string();
    }
    // Dhan's 2024 layout puts the number before its label.
    let before = Regex::new(r"(?i)(\d{5,})\s+contract\s*note\s*no").expect("valid");
    before
        .captures(stream)
        .map(|c| c[1].to_string())
        .unwrap_or_default()
}

/// The last number after each place `label` matches, summed over matches.
fn labelled_amounts(tokens: &[&str], stream_lower: &str, label: &Regex) -> Vec<Decimal> {
    // Map character offsets in the joined stream back to token indexes.
    let mut starts = Vec::with_capacity(tokens.len());
    let mut offset = 0;
    for token in tokens {
        starts.push(offset);
        offset += token.len() + 1;
    }
    let mut amounts = Vec::new();
    for m in label.find_iter(stream_lower) {
        let end = m.end();
        let index = starts.partition_point(|&s| s < end);
        let run = amount_run(tokens, index);
        if let Some(last) = run.last() {
            amounts.push(*last);
        }
    }
    amounts
}

/// The net amount the client pays or receives: the last number after a
/// "net amount ... payable/receivable ... client" label (the total column).
fn net_amount(tokens: &[&str]) -> Option<Decimal> {
    for i in 0..tokens.len().saturating_sub(1) {
        if !(tokens[i].eq_ignore_ascii_case("net") && tokens[i + 1].eq_ignore_ascii_case("amount"))
        {
            continue;
        }
        let mut words = String::new();
        let mut j = i + 2;
        while j < tokens.len() && j < i + 16 && !is_number(tokens[j]) {
            words.push_str(&tokens[j].to_lowercase());
            words.push(' ');
            j += 1;
        }
        if !(words.contains("payable") || words.contains("receivable")) {
            continue;
        }
        let run = amount_run(tokens, j);
        if let Some(amount) = run.iter().rev().map(|a| a.abs()).find(|a| !a.is_zero()) {
            return Some(amount);
        }
    }
    None
}

/// Reads a contract note from its extracted text.
pub fn read_contract_note(text: &str) -> ContractNote {
    let stream = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = stream.to_lowercase();
    let tokens: Vec<&str> = stream.split(' ').collect();
    let isin_re = isin_regex();
    let mut warnings = Vec::new();

    let starts = trade_starts(&tokens);
    let mut trades = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts
            .get(n + 1)
            .copied()
            .unwrap_or(tokens.len())
            .min(start + 80);
        match read_trade(&tokens[start..end], &isin_re) {
            Some(trade) => trades.push(trade),
            None => warnings.push(format!(
                "Could not read the trade at order {} {}",
                tokens[start],
                tokens[start + 1]
            )),
        }
    }
    let by_symbol = isins_by_symbol(&stream, &isin_re);
    for trade in &mut trades {
        if trade.isin.is_none() && !trade.symbol.is_empty() {
            trade.isin = by_symbol.get(&trade.symbol).cloned();
        }
        if trade.isin.is_none() {
            warnings.push(format!("No ISIN found for {} {}", trade.symbol, trade.name));
        }
    }

    // STT, each line of it ([STT-DEL.], [STT-RND], ...), signed as printed.
    let stt_label = Regex::new(r"securities\s+transactions?\s+tax(\s*\(rs\.?\))?|\[stt-[^\]]*\]")
        .expect("valid");
    let stt: Decimal = labelled_amounts(&tokens, &lower, &stt_label)
        .into_iter()
        .sum();

    let net = net_amount(&tokens);

    let traded: Decimal = trades
        .iter()
        .map(|t| match t.side {
            Side::Buy => t.value,
            Side::Sell => -t.value,
        })
        .sum();
    let turnover: Decimal = trades.iter().map(|t| t.value).sum();
    let charges = match net {
        Some(net) => {
            // Payable when the client bought more than sold, else receivable.
            let payable = net - traded;
            let receivable = -traded - net;
            let limit = turnover * Decimal::new(3, 2) + Decimal::new(100, 0);
            let fits = |c: Decimal| c >= -Decimal::new(1, 2) && c <= limit;
            match (fits(payable), fits(receivable)) {
                (true, false) => payable,
                (false, true) => receivable,
                (true, true) => payable.min(receivable).max(stt),
                (false, false) => {
                    warnings
                        .push("The net amount does not match the trades; charges left out".into());
                    stt
                }
            }
        }
        None => {
            if !trades.is_empty() {
                warnings.push("No net amount found; only STT is counted as a charge".into());
            }
            stt
        }
    };
    let fees = (charges - stt).max(Decimal::ZERO);

    ContractNote {
        broker: detect_broker(&lower).to_string(),
        number: note_number_of(&stream),
        trade_date: trade_date_of(&stream),
        trades,
        stt: stt.max(Decimal::ZERO),
        fees,
        warnings,
    }
}

/// Shares `total` across `weights`, rounded to paise, the last taking the
/// rounding.
fn share_out(total: Decimal, weights: &[Decimal]) -> Vec<Decimal> {
    let sum: Decimal = weights.iter().sum();
    let mut shares = Vec::with_capacity(weights.len());
    let mut given = Decimal::ZERO;
    for (i, w) in weights.iter().enumerate() {
        let share = if i + 1 == weights.len() {
            total - given
        } else if sum.is_zero() {
            Decimal::ZERO
        } else {
            (total * *w / sum).round_dp(2)
        };
        given += share;
        shares.push(share);
    }
    shares
}

/// Parses a contract note into rows: one per security and side, its fills
/// combined, with the note's fee and STT shared out by value.
pub fn parse_contract_note(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let note = read_contract_note(text);
    let headers = contract_note_headers();

    // Combine fills of one security on one side, in order of first fill.
    let mut groups: Vec<(String, Side, Vec<&ContractNoteTrade>)> = Vec::new();
    for trade in &note.trades {
        let key = trade
            .isin
            .clone()
            .unwrap_or_else(|| format!("{}|{}", trade.symbol, trade.name));
        match groups
            .iter_mut()
            .find(|(k, s, _)| *k == key && *s == trade.side)
        {
            Some((_, _, fills)) => fills.push(trade),
            None => groups.push((key, trade.side, vec![trade])),
        }
    }
    let values: Vec<Decimal> = groups
        .iter()
        .map(|(_, _, fills)| fills.iter().map(|f| f.value).sum())
        .collect();
    let fees = share_out(note.fees, &values);
    let taxes = share_out(note.stt, &values);
    let date = note
        .trade_date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();

    let rows = groups
        .iter()
        .enumerate()
        .map(|(i, (_, side, fills))| {
            let first = fills[0];
            let time = fills.iter().map(|f| f.time.as_str()).min().unwrap_or("");
            let quantity: Decimal = fills.iter().map(|f| f.quantity).sum();
            let value = values[i];
            let price = if quantity.is_zero() {
                Decimal::ZERO
            } else {
                (value / quantity).round_dp(6)
            };
            let symbol = if first.symbol.is_empty() {
                first.isin.clone().unwrap_or_default()
            } else {
                first.symbol.clone()
            };
            let mut comment = format!("{} contract note {}", note.broker, note.number);
            if fills.len() > 1 {
                comment.push_str(&format!(", {} fills", fills.len()));
            }
            vec![
                if date.is_empty() {
                    String::new()
                } else {
                    format!("{date} {time}")
                },
                note.broker.clone(),
                note.number.clone(),
                side.label().to_string(),
                symbol,
                first.isin.clone().unwrap_or_default(),
                first.name.clone(),
                quantity.normalize().to_string(),
                price.normalize().to_string(),
                value.round_dp(2).to_string(),
                fees[i].to_string(),
                taxes[i].to_string(),
                "INR".to_string(),
                comment,
            ]
        })
        .collect();
    (headers, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // Synthetic notes: invented trades, laid out as each broker's PDF text
    // comes out of the extractor, one layout per test.

    const ZERODHA_2020: &str = "CONTRACT NOTE CUM TAX INVOICE ZERODHA BROKING LIMITED
CONTRACT NOTE NO.: CNT-20/21-100000001 Equity TRADE DATE 2020-12-30 30/12/2020 SETTLEMENT NO 2020251
Remarks Equity 1000000000000001 10:31:44 1290001 10:31:44 ABCLTD / INE000A01011 B NSE 2 500.00 500.0000 (1000.00)
Sub Total: 2 (1000.00) 1000000000000002 10:30:01 1290002 10:30:01 XYZCO / INE000B01012 S NSE 10 100.00 100.0000 1000.00
Sub Total: 10 1000.00 1000000000000003 10:35:01 1290003 10:35:01 ABCLTD / INE000A01011 B NSE 2 510.00 510.0000 (1020.00)
Sub Total: 2 (1020.00) Net Total: (1020.00) Equity Futures and Options NET TOTAL
PAY IN/ PAY OUT OBLIGATION (1020.00) (1020.00) Exchange Transaction Charges (0.07) (0.07)
IGST (@18% of Brok, Trans & Clearing Charges) 4 (0.01) (0.01) UTT 4 Securities Transaction Tax (3.00) (3.0)
Stamp Duty (1.00) (1.00) Net amount receivable by Client / (payable by Client) (1024.08) (1024.08) 1 Applicable only";

    const ZERODHA_2026: &str = "Contract note cum tax invoice Zerodha Broking Limited Contract Note No: CNT-26/27-100000002 NCL-Cash
Trade Date: 09/11/2026 Settlement No: 2026212 ISIN Security Name / Symbol
INE000C01013 PQR LIMITED 3 964.8000 964.8000 2894.40
Pay in/Pay out obligation ( ₹ ) 2894.40 0.00 2894.40 Exchange transaction charges ( ₹ ) 4 (0.09) (0.09)
Securities transaction tax ( ₹ ) (3.00) (3.00) Stamp duty ( ₹ )
Net amount receivable/(payable by client) ( ₹ ) 2891.31 0.00 2891.31
Remarks 1100000000000009 12:21:12 206900001 12:21:12 PQR-EQ/INE000C01013 S NSE 1 0.0000 964.85 964.85
1100000000000009 12:21:12 206900002 12:21:12 PQR-EQ/INE000C01013 S NSE 2 0.0000 964.775 1929.55";

    const UPSTOX_2020: &str = "CONTRACT NOTE CUM TAX INVOICE RKSV SECURITIES INDIA PVT. LTD.
Contract Note No. : 17000001 Trade Date : 22/12/2020 Trading Code : TEST0001
Rem arks EQ-CASH 1200000000000001 09:21:27 50200001 09:21:27 BIRLA SOFT [INE000D01014] B 5 200.0000 0.0000 200.0000 1,000.00 NSE
1200000000000002 09:22:22 50200002 09:28:06 BIRLA SOFT [INE000D01014] B 5 210.0000 0.0000 210.0000 1,050.00 NSE
* NET BUY DELIVERY * 10 2,050.00 0 0 Brokerage B 1 0.0000 0.0100 0.0100 0.01 * SQUP SUMMARY * 1 0.01
(+) PAY IN/ (-) PAY OUT OBLIGATION 2,050.01 [IGST 18% On Charges] 0.01 0.01 [STT-DEL.] 2.05 2.05
[STT-RND] -0.05 -0.05 [TURNOVER CHG*] 0.07 0.07
Net Amount (-) receivable by client / (+) payable by Client 2,052.09 2,052.09";

    const UPSTOX_2026: &str = "UPSTOX SECURITIES PRIVATE LIMITED CONTRACT NOTE CUM TAX INVOICE Contract Note No. : 83000001
Trading Code: TEST0001 Trade Date : 16/01/2026 INE000E01015 EMS LIMITED -10 384.0000 384.0000 -3840.00
PAY IN / ( PAY OUT ) OBLIGATION -3840.00 -3840.00 Brokerage Charges 20.00 20.00 [IGST 18% On Brokerage] 3.60 3.60
[STT-DEL.] 3.84 3.84 [STT-Round off] 0.16 0.16 [TURNOVER CHG] 0.12 0.12
Net amount (-) receivable by Client / (+) payable by Client (Rs.) -3812.28 -3812.28
Annexure A EQ-CASH 1100000000000005 09:23:16 200800001 09:23:16 EMS LIMITED [INE000E01015] S -4 383.0000 383.0000 -1532.00 NSE
1100000000000006 14:40:53 200800002 14:40:53 EMS LIMITED [INE000E01015] S -6 384.6667 384.6667 -2308.00 NSE
1100000000000005 Brokerage Charges B 1 0.0000 20.0000 20.00";

    const DHAN_2023: &str = "MONEYLICIOUS SECURITIES PVT. LTD CONTRACT NOTE NO. 0700001 SETTLEMENT NO. NA 2023094
TRADE DATE May 22 2023 BSE Capital NCL Capital (2023094 - 23/05/2023 - Ref. No: 0184881)
1300000000000001 11:33:46 76900001 11:33:46 SIEMENS LTD BUY 1 D 3000.0000 0.0000 3000.0000 0.0000 3.00 -3000.0000 NSE-M
ISIN: INE000F01016 Net Total 1 -3000.00 NCL CAPITAL TOTAL(NET) PAY IN / PAY OUT OBLIGATION -3000.00 -3000.00
Securities Transaction Tax 3.00 3.00 IGST (@18.00%) 0.02 0.02 Exchange Transaction Charges *** 0.10 0.10
Stamp Duty 1.00 1.00 Net Amount Payable by Client -3004.12 -3004.12 Page No: 1";

    const DHAN_2024: &str = "Dhan dhan.co 18600001 CONTRACT NOTE NO : Name of the Client : TEST Trade Date : 28/11/2024 TEST0001
NCLCM M 2024225 110000003700001 12:30:34 205700001 12:30:34 (DIVISLAB NSE) Divi s Laboratories Limited B 1 6000.0000 6000.0000 -6000.00 D
Divi s Laboratories Limited 1 6000.0000 -6000.00 Symbol :DIVISLAB ISIN : INE000G01017 Net 1 @ 6000.0000 = 6000.00 Total Total Sell :
120000003900001 12:30:51 406000001 12:30:51 (KIMS NSE) Krishna Institute of Medical Sciences Limited S 2 500.0000 500.0000 1000.00 D
Symbol :KIMS ISIN : INE000H01018 Net 2 @ 500.0000 = 1000.00 Total
PAY IN/PAY OUT OBLIGATION 5000.00 DR 0.00 CR 5000.00 DR SECURITIES TRANSACTIONS TAX (RS.) 7.00 DR 0.00 CR 7.00 DR
STAMP DUTY (RS.) 1.00 DR 0.00 CR 1.00 DR NET AMOUNT RECEIVABLE/PAYABLE BY 5008.20 DR 0.00 CR 5008.20 DR LEDGER BALANCE 172.61 CR";

    const DHAN_2026: &str = "Dhan help@dhan.co Depository Participant of CDSL Contract Note No: 4300001 Trade Date: 18-05-2026
INE000J01019 APOLLOHOSP 1 8,000.0000 8,000.0000 8,000.00 -1 8,000.00
Pay In/Pay Out Obligation 8,000.00 CR 0.00 8,000.00 CR Securities Transactions Tax (Rs.) 8.00 DR 0.00 8.00 DR
Net Amount Receivable (CR) / Payable (DR) By Client 7,991.70 CR 0.00 7,991.70 CR Ledger Balance 229,026.98 CR
Quantity Price Net Rate Net Amount Remark NCL-NSE-Equity-M 1000000031100001 10:52:34 3037901 10:52:34 APOLLOHOSP-APOLLO HOSPITALS ENTERPRISE LT S 1 8,000.0000 8,000.0000 8,000.00 D";

    fn row(rows: &[Vec<String>], i: usize) -> HashMap<String, String> {
        contract_note_headers()
            .into_iter()
            .zip(rows[i].iter().cloned())
            .collect()
    }

    #[test]
    fn recognises_contract_notes_before_depository_statements() {
        for text in [
            ZERODHA_2020,
            ZERODHA_2026,
            UPSTOX_2020,
            UPSTOX_2026,
            DHAN_2023,
            DHAN_2024,
            DHAN_2026,
        ] {
            assert!(is_contract_note(text), "{text}");
        }
        assert!(!is_contract_note(
            "Central Depository Services (India) Limited BO ID: 1208"
        ));
    }

    #[test]
    fn zerodha_2020_combines_fills_and_shares_out_charges() {
        let note = read_contract_note(ZERODHA_2020);
        assert_eq!(note.broker, "Zerodha");
        assert_eq!(note.number, "CNT-20/21-100000001");
        assert_eq!(note.trade_date, NaiveDate::from_ymd_opt(2020, 12, 30));
        assert_eq!(note.trades.len(), 3);
        assert_eq!(note.stt, dec!(3.0));
        // Paid 1024.08 for a net purchase of 1020.00: 4.08 of charges, 3 STT.
        assert_eq!(note.fees, dec!(1.08));
        assert!(note.warnings.is_empty(), "{:?}", note.warnings);

        let (headers, rows) = parse_contract_note(ZERODHA_2020);
        assert_eq!(headers, contract_note_headers());
        assert_eq!(rows.len(), 2);
        let buy = row(&rows, 0);
        assert_eq!(buy["Date"], "2020-12-30 10:31:44");
        assert_eq!(buy["Side"], "BUY");
        assert_eq!(buy["Symbol"], "ABCLTD");
        assert_eq!(buy["ISIN"], "INE000A01011");
        assert_eq!(buy["Quantity"], "4");
        assert_eq!(buy["Price"], "505");
        assert_eq!(buy["Value"], "2020.00");
        let sell = row(&rows, 1);
        assert_eq!(sell["Side"], "SELL");
        assert_eq!(sell["Value"], "1000.00");
        let fee: Decimal = rows
            .iter()
            .map(|r| Decimal::from_str(&r[10]).unwrap_or_default())
            .sum();
        let tax: Decimal = rows
            .iter()
            .map(|r| Decimal::from_str(&r[11]).unwrap_or_default())
            .sum();
        assert_eq!(fee, dec!(1.08));
        assert_eq!(tax, dec!(3.0));
        // 3.0 x 2020 / 3020, to the paisa.
        assert_eq!(buy["Tax"], "2.01");
    }

    #[test]
    fn zerodha_2026_reads_trades_without_a_gross_rate_column() {
        let note = read_contract_note(ZERODHA_2026);
        assert_eq!(note.trades.len(), 2);
        assert_eq!(note.trades[1].value, dec!(1929.55));
        assert_eq!(note.stt, dec!(3.00));
        // Sold 2894.40, received 2891.31.
        assert_eq!(note.fees, dec!(0.09));
        let (_, rows) = parse_contract_note(ZERODHA_2026);
        assert_eq!(rows.len(), 1);
        assert_eq!(row(&rows, 0)["Symbol"], "PQR");
        assert_eq!(row(&rows, 0)["Quantity"], "3");
        assert_eq!(row(&rows, 0)["Price"], "964.8");
    }

    #[test]
    fn upstox_2020_counts_the_brokerage_line_and_signed_stt_round_off() {
        let note = read_contract_note(UPSTOX_2020);
        assert_eq!(note.broker, "Upstox");
        assert_eq!(note.number, "17000001");
        assert_eq!(note.trades.len(), 2);
        assert_eq!(note.stt, dec!(2.00));
        // 2052.09 - 2050.00 = 2.09, of which 2.00 STT.
        assert_eq!(note.fees, dec!(0.09));
        let (_, rows) = parse_contract_note(UPSTOX_2020);
        assert_eq!(rows.len(), 1);
        let buy = row(&rows, 0);
        assert_eq!(buy["ISIN"], "INE000D01014");
        assert_eq!(buy["Name"], "BIRLA SOFT");
        assert_eq!(buy["Symbol"], "INE000D01014");
        assert_eq!(buy["Price"], "205");
    }

    #[test]
    fn upstox_2026_reads_the_annexure_with_negative_sell_quantities() {
        let note = read_contract_note(UPSTOX_2026);
        assert_eq!(note.trades.len(), 2);
        assert!(note.trades.iter().all(|t| t.side == Side::Sell));
        assert_eq!(note.trades[0].quantity, dec!(4));
        assert_eq!(note.stt, dec!(4.00));
        // Sold 3840.00, received 3812.28: 27.72, of which 4.00 STT.
        assert_eq!(note.fees, dec!(23.72));
        let (_, rows) = parse_contract_note(UPSTOX_2026);
        assert_eq!(row(&rows, 0)["Quantity"], "10");
        assert_eq!(row(&rows, 0)["Value"], "3840.00");
    }

    #[test]
    fn dhan_2023_finds_the_isin_after_the_trade() {
        let note = read_contract_note(DHAN_2023);
        assert_eq!(note.broker, "Dhan");
        assert_eq!(note.number, "0700001");
        assert_eq!(note.trade_date, NaiveDate::from_ymd_opt(2023, 5, 22));
        assert_eq!(note.trades.len(), 1);
        assert_eq!(note.trades[0].isin.as_deref(), Some("INE000F01016"));
        assert_eq!(note.trades[0].side, Side::Buy);
        assert_eq!(note.stt, dec!(3.00));
        assert_eq!(note.fees, dec!(1.12));
    }

    #[test]
    fn dhan_2024_reads_symbols_and_a_number_before_its_label() {
        let note = read_contract_note(DHAN_2024);
        assert_eq!(note.number, "18600001");
        assert_eq!(note.trade_date, NaiveDate::from_ymd_opt(2024, 11, 28));
        assert_eq!(note.trades.len(), 2);
        assert_eq!(note.trades[0].symbol, "DIVISLAB");
        assert_eq!(note.trades[0].isin.as_deref(), Some("INE000G01017"));
        assert_eq!(note.trades[1].symbol, "KIMS");
        assert_eq!(note.trades[1].isin.as_deref(), Some("INE000H01018"));
        assert_eq!(note.trades[1].side, Side::Sell);
        assert_eq!(note.stt, dec!(7.00));
        // Net purchase 5000.00, paid 5008.20.
        assert_eq!(note.fees, dec!(1.20));
    }

    #[test]
    fn dhan_2026_takes_the_isin_from_the_summary() {
        let note = read_contract_note(DHAN_2026);
        assert_eq!(note.trades.len(), 1);
        assert_eq!(note.trades[0].symbol, "APOLLOHOSP");
        assert_eq!(note.trades[0].isin.as_deref(), Some("INE000J01019"));
        assert_eq!(note.stt, dec!(8.00));
        assert_eq!(note.fees, dec!(0.30));
    }

    #[test]
    fn a_note_without_trades_gives_no_rows() {
        let (_, rows) = parse_contract_note("Contract note Trade date: 01/01/2024 nothing traded");
        assert!(rows.is_empty());
    }
}
