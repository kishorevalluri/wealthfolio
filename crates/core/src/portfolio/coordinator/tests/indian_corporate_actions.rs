//! Corporate actions through the engine and into Indian capital gains: the
//! lots a split, a bonus issue and an amalgamation leave behind must carry
//! the purchase date and cost the Income-tax Act gives them.

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal_macros::dec;

use super::{harness, request};
use crate::lots::{
    calculate_indian_capital_gains, IndianAssetTaxClassification, IndianLossesBroughtForward,
    IndianTaxSection, IndianTaxYearSummary,
};
use crate::portfolio::coordinator::SilentObserver;
use crate::test_support::scenario::Scenario;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

/// Runs the scenario and works out financial year `fy` for its one account,
/// with every asset a listed share and `fmv` the 31 Jan 2018 prices.
async fn tax_year(
    yaml: &str,
    fmv: &[(&str, rust_decimal::Decimal)],
    fy: i32,
) -> IndianTaxYearSummary {
    let scenario: Scenario = serde_yaml::from_str(yaml).expect("scenario parses");
    let facts = scenario.facts();
    let account = facts.accounts[0].id.clone();
    let classes: HashMap<String, IndianAssetTaxClassification> = facts
        .assets
        .iter()
        .map(|a| (a.id.clone(), IndianAssetTaxClassification::EquityListed))
        .collect();
    let harness = harness(facts).await;
    let report = harness
        .coordinator
        .run_job(request(), &SilentObserver)
        .await
        .expect("job runs");
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let lots = harness
        .lot_repo
        .get_all_lots_for_account(&account)
        .await
        .expect("lots");
    let open_dates: HashMap<String, NaiveDate> = lots
        .iter()
        .map(|lot| {
            let day =
                NaiveDate::parse_from_str(&lot.open_date[..10], "%Y-%m-%d").expect("lot open date");
            (lot.id.clone(), day)
        })
        .collect();
    let disposals: Vec<_> = harness
        .lot_repo
        .get_lot_disposals_for_account(&account)
        .await
        .expect("disposals")
        .into_iter()
        .filter(|d| {
            let day = NaiveDate::parse_from_str(&d.disposal_date[..10], "%Y-%m-%d")
                .expect("disposal date");
            day >= date(fy, 4, 1) && day <= date(fy + 1, 3, 31)
        })
        .collect();
    let fmv: HashMap<String, rust_decimal::Decimal> =
        fmv.iter().map(|(id, v)| (id.to_string(), *v)).collect();
    calculate_indian_capital_gains(
        &disposals,
        &open_dates,
        &classes,
        &fmv,
        &HashMap::new(),
        &HashMap::new(),
        IndianLossesBroughtForward::default(),
        fy,
    )
}

/// 100 shares of A bought in 2017 for ₹1,00,000. A splits 2 for 1 (200
/// shares), issues a 1:1 bonus in 2020 (400 shares), and in 2023 is
/// amalgamated into B at 1 B for 4 A (100 B). 75 B are sold in June 2025 at
/// ₹6,000.
///
/// FIFO takes the 50 B that came from the 2017 purchase, then 25 of the 50
/// bonus B. Section 2(42A) counts the amalgamating company's shares in the
/// holding period and Section 49(2) keeps their cost, so the first 50 are a
/// 2017 purchase at ₹1,00,000, grandfathered at the 31 Jan 2018 price. That
/// price was ₹1,400 for a share of A as it then was; one B stands for two of
/// those, so it is ₹2,800 a B. Bonus shares cost nil and run from allotment
/// (Section 55(2)(aa)(iiia)).
#[tokio::test]
async fn split_bonus_and_amalgamation_keep_dates_and_cost() {
    let summary = tax_year(
        r#"
id: IN-CA-01
title: Split, bonus and amalgamation
policy: { base_currency: INR, timezone: Asia/Kolkata, as_of: 2025-06-30 }
accounts:
  - { id: demat, name: Demat, currency: INR }
assets:
  - { id: a, quote_ccy: INR }
  - { id: b, quote_ccy: INR }
activities:
  - { id: dep, account: demat, type: DEPOSIT, date: 2017-05-31, amount: 100000 }
  - { id: buy, account: demat, type: BUY, date: 2017-06-01, asset: a, quantity: 100, unit_price: 1000, amount: 100000 }
  - { id: split, account: demat, type: SPLIT, date: 2019-07-01, asset: a, quantity: 2 }
  - { id: bonus, account: demat, type: BUY, date: 2020-09-01, asset: a, quantity: 200, unit_price: 0, amount: 0 }
  - { id: out, account: demat, type: ADJUSTMENT, subtype: CONVERSION_OUT, date: 2023-03-01, asset: a, quantity: 400, source_group_id: merger }
  - { id: in, account: demat, type: ADJUSTMENT, subtype: CONVERSION_IN, date: 2023-03-01, asset: b, quantity: 100, source_group_id: merger }
  - { id: sell, account: demat, type: SELL, date: 2025-06-02, asset: b, quantity: 75, unit_price: 6000, amount: 450000 }
quotes:
  - { asset: a, day: 2017-06-01, close: 1000 }
  - { asset: a, day: 2023-03-01, close: 1500 }
  - { asset: b, day: 2023-03-01, close: 6000 }
  - { asset: b, day: 2025-06-02, close: 6000 }
"#,
        &[("b", dec!(2800))],
        2025,
    )
    .await;

    assert_eq!(summary.calculations.len(), 2, "{:#?}", summary.calculations);
    let bought = &summary.calculations[0];
    assert_eq!(bought.acquisition_date, date(2017, 6, 1));
    assert_eq!(bought.quantity, dec!(50));
    assert_eq!(bought.actual_cost, dec!(100000));
    assert_eq!(bought.proceeds, dec!(300000));
    assert_eq!(bought.tax_section, IndianTaxSection::Sec112a);
    // max(1,00,000, min(2,800 x 50, 3,00,000)) = 1,40,000
    assert_eq!(bought.deemed_cost, dec!(140000));
    assert_eq!(bought.gross_gain, dec!(160000));

    let bonus = &summary.calculations[1];
    assert_eq!(bonus.acquisition_date, date(2020, 9, 1));
    assert_eq!(bonus.quantity, dec!(25));
    assert_eq!(bonus.actual_cost, dec!(0));
    assert_eq!(bonus.tax_section, IndianTaxSection::Sec112a);
    assert_eq!(bonus.gross_gain, dec!(150000));

    assert_eq!(summary.sec_112a_ltcg_gross, dec!(310000));
    assert_eq!(summary.sec_112a_exemption_applied, dec!(125000));
    assert_eq!(summary.sec_112a_ltcg_taxable, dec!(185000));
}
