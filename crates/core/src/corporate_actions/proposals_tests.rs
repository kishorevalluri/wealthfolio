use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal_macros::dec;
use serde_json::json;

use super::nse::parse_nse_actions;
use super::*;
use crate::accounts::{Account, TrackingMode};
use crate::activities::Activity;
use crate::assets::Asset;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn account(id: &str) -> Account {
    Account {
        id: id.into(),
        name: format!("Broker {id}"),
        currency: "INR".into(),
        is_active: true,
        tracking_mode: TrackingMode::Transactions,
        ..Account::default()
    }
}

fn asset(id: &str, symbol: &str) -> Asset {
    serde_json::from_value(json!({
        "id": id, "kind": "INVESTMENT", "name": format!("{symbol} Ltd"),
        "quoteMode": "MANUAL", "quoteCcy": "INR", "instrumentType": "EQUITY",
        "instrumentSymbol": symbol, "instrumentExchangeMic": "XNSE",
        "createdAt": "2020-01-01T00:00:00", "updatedAt": "2020-01-01T00:00:00"
    }))
    .unwrap()
}

fn act(account: &str, asset: &str, ty: &str, date: &str, qty: &str, price: &str) -> Activity {
    let mut value = json!({
        "id": format!("{account}-{asset}-{ty}-{date}-{qty}"), "accountId": account,
        "assetId": asset, "activityType": ty, "status": "POSTED",
        "activityDate": format!("{date}T00:00:00Z"), "currency": "INR",
        "createdAt": "2020-01-01T00:00:00Z", "updatedAt": "2020-01-01T00:00:00Z"
    });
    if ty == "SPLIT" {
        value["amount"] = json!(qty);
    } else {
        value["quantity"] = json!(qty);
        value["unitPrice"] = json!(price);
        value["amount"] =
            json!((qty.parse::<f64>().unwrap() * price.parse::<f64>().unwrap()).to_string());
    }
    serde_json::from_value(value).unwrap()
}

fn action(symbol: &str, subject: &str, ex: &str) -> ExchangeAction {
    ExchangeAction {
        symbol: symbol.into(),
        isin: None,
        company: None,
        subject: subject.into(),
        ex_date: d(ex),
        record_date: Some(d(ex)),
    }
}

#[test]
fn reads_nse_subjects() {
    assert_eq!(
        parse_subject("Bonus 1:1"),
        ActionKind::Bonus {
            new: dec!(1),
            held: dec!(1)
        }
    );
    assert_eq!(
        parse_subject("Bonus 2:5"),
        ActionKind::Bonus {
            new: dec!(2),
            held: dec!(5)
        }
    );
    assert_eq!(
        parse_subject(
            "Face Value Split (Sub-Division) - From Rs 10/- Per Share To Rs 2/- Per Share"
        ),
        ActionKind::Split { ratio: dec!(5) }
    );
    assert_eq!(
        parse_subject(
            "Face Value Split (Sub-Division) - From Re 1/- Per Share To Re 0.50/- Per Share"
        ),
        ActionKind::Split { ratio: dec!(2) }
    );
    assert_eq!(
        parse_subject("Consolidation Of Shares From Rs 1/- Per Share To Rs 10/- Per Share"),
        ActionKind::Split { ratio: dec!(0.1) }
    );
    assert_eq!(parse_subject("Demerger"), ActionKind::NeedsReview);
    assert_eq!(
        parse_subject("Scheme Of Arrangement"),
        ActionKind::NeedsReview
    );
    assert_eq!(
        parse_subject("Interim Dividend - Rs 7 Per Share"),
        ActionKind::Ignored
    );
    assert_eq!(parse_subject("Annual General Meeting"), ActionKind::Ignored);
    assert_eq!(parse_subject("Buyback"), ActionKind::Ignored);
}

#[test]
fn reads_nse_reply_rows() {
    let body = json!([
        {"symbol": "EXAMPLE", "series": "EQ", "faceVal": "10", "subject": "Bonus 1:1",
         "exDate": "26-Nov-2025", "recDate": "26-Nov-2025", "comp": "Example Ltd", "isin": "INE000A01011"},
        {"symbol": "EXAMPLE", "series": "EQ", "subject": "Dividend - Rs 5", "exDate": "-", "recDate": "-"},
        {"symbol": "EXAMPLE", "series": "N1", "subject": "Interest Payment", "exDate": "01-Jan-2026"}
    ]);
    let rows = parse_nse_actions(&body, "EXAMPLE");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ex_date, d("2025-11-26"));
    assert_eq!(rows[0].record_date, Some(d("2025-11-26")));
    assert_eq!(rows[0].isin.as_deref(), Some("INE000A01011"));
    assert_eq!(rows[0].company.as_deref(), Some("Example Ltd"));
    assert!(parse_nse_actions(&json!({"data": []}), "X").is_empty());
}

#[test]
fn proposes_missing_bonuses_and_splits_for_shares_held_at_the_ex_date() {
    let accounts = vec![account("z")];
    let assets = vec![
        asset("amc", "AMC"),
        asset("bld", "BLD"),
        asset("old", "OLD"),
    ];
    let activities = vec![
        act("z", "amc", "BUY", "2024-01-10", "12", "3000"),
        act("z", "amc", "BUY", "2024-06-10", "5", "3700"),
        // Bought on the ex-date: not entitled.
        act("z", "amc", "BUY", "2025-11-26", "3", "2600"),
        act("z", "bld", "BUY", "2026-02-01", "70", "800"),
        // Sold out before its bonus.
        act("z", "old", "BUY", "2020-01-01", "10", "100"),
        act("z", "old", "SELL", "2021-01-01", "10", "120"),
    ];
    let actions = HashMap::from([
        (
            "AMC".to_string(),
            vec![action("AMC", "Bonus 1:1", "2025-11-26")],
        ),
        (
            "BLD".to_string(),
            vec![
                action(
                    "BLD",
                    "Face Value Split (Sub-Division) - From Rs 10/- Per Share To Rs 2/- Per Share",
                    "2026-10-08",
                ),
                action("BLD", "Final Dividend - Rs 2", "2026-08-01"),
                // Not yet happened.
                action("BLD", "Bonus 1:2", "2027-01-15"),
            ],
        ),
        (
            "OLD".to_string(),
            vec![action("OLD", "Bonus 1:1", "2022-01-01")],
        ),
    ]);
    let (proposals, recorded) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert_eq!(recorded, 0);
    assert_eq!(proposals.len(), 2, "{proposals:#?}");
    assert_eq!(proposals[0].kind, ProposalKind::Bonus);
    assert_eq!(proposals[0].symbol, "AMC");
    assert_eq!(proposals[0].held_quantity, dec!(17));
    assert_eq!(proposals[0].bonus_quantity, Some(dec!(17)));
    assert_eq!(proposals[1].kind, ProposalKind::Split);
    assert_eq!(proposals[1].split_ratio, Some(dec!(5)));
    assert_eq!(proposals[1].held_quantity, dec!(70));

    let symbols = symbols_to_check(&accounts, &assets, &activities, d("2026-11-23"));
    assert_eq!(symbols, vec!["AMC".to_string(), "BLD".to_string()]);
}

#[test]
fn skips_actions_already_recorded() {
    let accounts = vec![account("z")];
    let assets = vec![asset("amc", "AMC"), asset("bld", "BLD"), asset("cv", "CV")];
    let activities = vec![
        act("z", "amc", "BUY", "2024-01-10", "17", "3000"),
        act("z", "amc", "BUY", "2025-11-27", "17", "0"),
        act("z", "bld", "BUY", "2026-02-01", "70", "800"),
        act("z", "bld", "SPLIT", "2026-10-08", "5", "0"),
        act("z", "cv", "BUY", "2025-01-01", "10", "100"),
    ];
    let mut conversion = act("z", "cv", "ADJUSTMENT", "2026-03-02", "10", "0");
    conversion.subtype = Some("CONVERSION_OUT".into());
    let mut activities = activities;
    activities.push(conversion);
    let actions = HashMap::from([
        (
            "AMC".to_string(),
            vec![action("AMC", "Bonus 1:1", "2025-11-26")],
        ),
        (
            "BLD".to_string(),
            vec![action(
                "BLD",
                "Face Value Split - From Rs 10/- To Rs 2/-",
                "2026-10-08",
            )],
        ),
        (
            "CV".to_string(),
            vec![action(
                "CV",
                "Face Value Split - From Rs 10/- To Rs 1/-",
                "2026-03-02",
            )],
        ),
    ]);
    let (proposals, recorded) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert!(proposals.is_empty(), "{proposals:#?}");
    assert_eq!(recorded, 3);
}

#[test]
fn flags_demergers_and_leaves_holdings_mode_accounts_alone() {
    let mut holdings_account = account("h");
    holdings_account.tracking_mode = TrackingMode::Holdings;
    let accounts = vec![account("z"), holdings_account];
    let assets = vec![asset("par", "PAR")];
    let activities = vec![
        act("z", "par", "BUY", "2024-01-10", "8", "1000"),
        act("h", "par", "BUY", "2024-01-10", "8", "1000"),
    ];
    let actions = HashMap::from([(
        "PAR".to_string(),
        vec![action(
            "PAR",
            "Scheme Of Arrangement (Demerger)",
            "2025-05-01",
        )],
    )]);
    let (proposals, _) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].kind, ProposalKind::Review);
    assert_eq!(proposals[0].account_id, "z");
    assert_eq!(proposals[0].held_quantity, dec!(8));
}

#[test]
fn bonus_fractions_are_dropped() {
    let accounts = vec![account("z")];
    let assets = vec![asset("frac", "FRAC")];
    let activities = vec![act("z", "frac", "BUY", "2024-01-10", "7", "100")];
    let actions = HashMap::from([(
        "FRAC".to_string(),
        vec![action("FRAC", "Bonus 1:2", "2025-01-01")],
    )]);
    let (proposals, _) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert_eq!(proposals[0].bonus_quantity, Some(dec!(3)));
}

/// NSE's reply for HDFCAMC as a browser received it on 23 Nov 2026 (public
/// market data, trimmed to four rows).
const NSE_HDFCAMC_REPLY: &str = r#"[
{"bcEndDate":"-","bcStartDate":"-","caBroadcastDate":null,"comp":"HDFC Asset Management Company Limited","exDate":"05-Jun-2026","faceVal":"5","ind":"-","isin":"INE127D01025","ndEndDate":"-","ndStartDate":"-","recDate":"05-Jun-2026","series":"EQ","subject":"Dividend - Rs 54 Per Share","symbol":"HDFCAMC"},
{"bcEndDate":"-","bcStartDate":"-","caBroadcastDate":null,"comp":"HDFC Asset Management Company Limited","exDate":"26-Nov-2025","faceVal":"5","ind":"-","isin":"INE127D01025","ndEndDate":"-","ndStartDate":"-","recDate":"26-Nov-2025","series":"EQ","subject":"Bonus 1:1","symbol":"HDFCAMC"},
{"bcEndDate":"29-Jun-2022","bcStartDate":"11-Jun-2022","caBroadcastDate":null,"comp":"HDFC Asset Management Company Limited","exDate":"09-Jun-2022","faceVal":"5","ind":"-","isin":"INE127D01025","ndEndDate":"-","ndStartDate":"-","recDate":"-","series":"EQ","subject":"Annual General Meeting/Dividend - Rs 42 Per Share","symbol":"HDFCAMC"},
{"bcEndDate":"-","bcStartDate":"-","caBroadcastDate":null,"comp":"HDFC Asset Management Company Limited","exDate":"06-Mar-2019","faceVal":"5","ind":"-","isin":"INE127D01025","ndEndDate":"-","ndStartDate":"-","recDate":"07-Mar-2019","series":"EQ","subject":" Interim Dividend - Rs 12 Per Share","symbol":"HDFCAMC"}
]"#;

#[test]
fn reads_a_real_nse_reply_and_proposes_its_bonus() {
    let body: serde_json::Value = serde_json::from_str(NSE_HDFCAMC_REPLY).unwrap();
    let rows = parse_nse_actions(&body, "HDFCAMC");
    assert_eq!(rows.len(), 4);
    let bonus = rows.iter().find(|r| r.subject == "Bonus 1:1").unwrap();
    assert_eq!(bonus.ex_date, d("2025-11-26"));
    assert_eq!(bonus.record_date, Some(d("2025-11-26")));
    assert_eq!(rows[2].record_date, None);

    let accounts = vec![account("z")];
    let assets = vec![asset("amc", "HDFCAMC")];
    let mut activities = vec![
        act("z", "amc", "BUY", "2024-01-10", "17", "3000"),
        act("z", "amc", "BUY", "2025-12-24", "1", "2669.6"),
    ];
    let actions = HashMap::from([("HDFCAMC".to_string(), rows)]);
    let (proposals, recorded) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert_eq!(recorded, 0);
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].bonus_quantity, Some(dec!(17)));
    assert_eq!(proposals[0].activity_date, d("2025-11-26"));

    // Once the bonus is in, nothing is proposed.
    activities.push(act("z", "amc", "BUY", "2025-11-27", "17", "0"));
    let (proposals, recorded) =
        propose_corporate_actions(&accounts, &assets, &activities, &actions, d("2026-11-23"));
    assert!(proposals.is_empty());
    assert_eq!(recorded, 1);
}
