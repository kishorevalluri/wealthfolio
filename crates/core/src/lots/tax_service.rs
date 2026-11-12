use std::collections::HashMap;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value;

use super::{
    calculate_indian_capital_gains, summarize_other_sources, IndianAssetTaxClassification,
    IndianDisposalCharges, IndianLossesBroughtForward, IndianTaxAssetInfo, IndianTaxYearSummary,
    LotDisposal, LotRecord, LotRepositoryTrait,
};
use crate::activities::{Activity, ActivityServiceTrait, ACTIVITY_SUBTYPE_BUYBACK};
use crate::assets::{Asset, AssetServiceTrait, InstrumentType};
use crate::errors::{Result, ValidationError};

/// The tax type set explicitly in the asset's metadata (`taxClassification`),
/// if any.
pub fn explicit_tax_classification(asset: &Asset) -> Option<IndianAssetTaxClassification> {
    let metadata = asset.metadata.as_ref()?;
    let s = metadata
        .get("taxClassification")
        .or_else(|| metadata.get("assetTaxClassification"))?
        .as_str()?;
    match s.to_lowercase().as_str() {
        "equity_listed" | "equitylisted" => Some(IndianAssetTaxClassification::EquityListed),
        "equity_mf" | "equitymf" => Some(IndianAssetTaxClassification::EquityMf),
        "reit_invit" | "reitinvit" => Some(IndianAssetTaxClassification::ReitInvIt),
        "debt_mf" | "debtmf" | "debt_mf_post_2023" | "debtmfpost2023" => {
            Some(IndianAssetTaxClassification::DebtMf)
        }
        "non_equity_etf" | "nonequityetf" => Some(IndianAssetTaxClassification::NonEquityEtf),
        "non_equity_fund" | "nonequityfund" => Some(IndianAssetTaxClassification::NonEquityFund),
        "unlisted_debenture_post_2024" | "unlisteddebenturepost2024" => {
            Some(IndianAssetTaxClassification::UnlistedDebenturePost2024)
        }
        "other_capital_asset" | "othercapitalasset" => {
            Some(IndianAssetTaxClassification::OtherCapitalAsset)
        }
        "gold_sgb" | "goldsgb" => Some(IndianAssetTaxClassification::GoldSgb),
        _ => None,
    }
}

/// Classifies an asset for Indian capital gains taxation based on its instrument type,
/// exchange MIC, and explicit metadata overrides.
pub fn classify_asset_for_indian_tax(asset: &Asset) -> IndianAssetTaxClassification {
    // 1. Explicit tax type in metadata wins
    if let Some(c) = explicit_tax_classification(asset) {
        return c;
    }

    // 2. Derive classification from instrument identity & metadata
    match asset.instrument_type {
        Some(InstrumentType::Equity) => {
            let name_lower = asset.name.as_deref().unwrap_or("").to_lowercase();
            let symbol_lower = asset
                .instrument_symbol
                .as_deref()
                .unwrap_or("")
                .to_lowercase();
            let mic = asset.instrument_exchange_mic.as_deref().unwrap_or("");

            if name_lower.contains("sovereign gold bond") || has_word(&name_lower, "sgb") {
                IndianAssetTaxClassification::GoldSgb
            } else if name_lower.contains("reit")
                || name_lower.contains("invit")
                || symbol_lower.contains("reit")
                || symbol_lower.contains("invit")
            {
                IndianAssetTaxClassification::ReitInvIt
            } else if mic == "AMFI"
                || mic == "MF"
                || name_lower.contains("fund")
                || name_lower.contains("mutual")
                || name_lower.contains("etf")
                || name_lower.contains("bees")
            {
                guess_fund_classification(&name_lower)
            } else {
                IndianAssetTaxClassification::EquityListed
            }
        }
        Some(InstrumentType::Metal) => IndianAssetTaxClassification::GoldSgb,
        Some(InstrumentType::Bond) => IndianAssetTaxClassification::OtherCapitalAsset,
        _ => IndianAssetTaxClassification::OtherCapitalAsset,
    }
}

/// Words in a fund's name that mark it as a debt fund.
const DEBT_FUND_WORDS: [&str; 14] = [
    "liquid",
    "1d rate",
    "overnight",
    "money market",
    "bharat bond",
    "gilt",
    "g-sec",
    "gsec",
    "sdl",
    "debt",
    "bond",
    "short term",
    "short duration",
    "ultra short",
];

/// Words in a fund's name that mark it as a gold, silver or international fund.
const NON_EQUITY_FUND_WORDS: [&str; 14] = [
    "gold",
    "silver",
    "nasdaq",
    "s&p",
    "hang seng",
    "msci",
    "nyse",
    "fang",
    "international",
    "overseas",
    "feeder",
    "u.s.",
    "japan",
    "taiwan",
];

fn has_word(name: &str, word: &str) -> bool {
    name.match_indices(word).any(|(at, _)| {
        let before = name[..at].chars().next_back();
        let after = name[at + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric())
    })
}

/// A fund's tax type guessed from its name (lower case): debt, gold or
/// international (listed when an ETF), else equity. Only a guess, so the tax
/// summary asks for it to be checked.
fn guess_fund_classification(name: &str) -> IndianAssetTaxClassification {
    let listed = has_word(name, "etf") || name.contains("bees");
    if NON_EQUITY_FUND_WORDS.iter().any(|w| has_word(name, w)) {
        if listed {
            IndianAssetTaxClassification::NonEquityEtf
        } else {
            IndianAssetTaxClassification::NonEquityFund
        }
    } else if DEBT_FUND_WORDS.iter().any(|w| has_word(name, w)) {
        IndianAssetTaxClassification::DebtMf
    } else {
        IndianAssetTaxClassification::EquityMf
    }
}

/// Extracts grandfathering Fair Market Value (FMV) as of January 31, 2018 from asset metadata.
pub fn extract_fmv_2018(asset: &Asset) -> Option<Decimal> {
    let metadata = asset.metadata.as_ref()?;
    let fmv_val = metadata
        .get("fmv20180131")
        .or_else(|| metadata.get("fmv_2018_01_31"))
        .or_else(|| metadata.get("fmv2018"));

    match fmv_val {
        Some(Value::Number(n)) => n.to_string().parse::<Decimal>().ok(),
        Some(Value::String(s)) => Decimal::from_str(s).ok(),
        _ => None,
    }
}

/// Computes Indian Tax Summary for the specified accounts and financial year.
///
fn decimal(text: &str) -> Decimal {
    Decimal::from_str(text.trim()).unwrap_or(Decimal::ZERO)
}

/// The share of `amount` that belongs to `part` of `whole` units (at most all of it).
fn share(amount: Decimal, part: Decimal, whole: Decimal) -> Decimal {
    if whole.is_zero() {
        return Decimal::ZERO;
    }
    amount.abs() * (part.abs() / whole.abs()).min(Decimal::ONE)
}

/// Fee and tax of the sell trade, and tax of the buy trade, for the units one
/// disposal sold, in base currency.
fn disposal_charges(
    d: &LotDisposal,
    lot: Option<&LotRecord>,
    sell: Option<&Activity>,
) -> IndianDisposalCharges {
    let qty = decimal(&d.quantity);
    let fx = Some(decimal(&d.fx_rate_to_base))
        .filter(|r| !r.is_zero())
        .unwrap_or(Decimal::ONE);
    let (sale_fee, sale_tax) = match sell {
        Some(trade) => {
            let traded = trade.quantity.unwrap_or_default();
            (
                share(trade.fee.unwrap_or_default(), qty, traded) * fx,
                share(trade.tax.unwrap_or_default(), qty, traded) * fx,
            )
        }
        None => (Decimal::ZERO, Decimal::ZERO),
    };
    let purchase_tax = lot
        .map(|lot| {
            share(
                decimal(&lot.tax_allocated_base),
                qty,
                decimal(&lot.original_quantity),
            )
        })
        .unwrap_or_default();
    let buyback = sell.is_some_and(|trade| {
        trade
            .subtype
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(ACTIVITY_SUBTYPE_BUYBACK))
    });
    IndianDisposalCharges {
        sale_fee,
        sale_tax,
        purchase_tax,
        buyback,
    }
}

/// Years a capital loss can be carried forward (Section 74).
const LOSS_CARRY_YEARS: i32 = 8;

/// Capital loss of one financial year not yet set off.
struct LossYear {
    year: i32,
    stcl: Decimal,
    ltcl: Decimal,
}

/// Takes the brought-forward losses `summary` set off from the oldest years
/// first, then adds `year`'s own unabsorbed loss.
fn carry_losses_forward(losses: &mut Vec<LossYear>, summary: &IndianTaxYearSummary, year: i32) {
    let mut st_used = summary.stcl_brought_forward_set_off;
    let mut lt_used = summary.ltcl_brought_forward_set_off;
    for loss in losses.iter_mut() {
        let st = st_used.min(loss.stcl);
        loss.stcl -= st;
        st_used -= st;
        let lt = lt_used.min(loss.ltcl);
        loss.ltcl -= lt;
        lt_used -= lt;
    }
    let st_left_before: Decimal = losses.iter().map(|l| l.stcl).sum();
    let lt_left_before: Decimal = losses.iter().map(|l| l.ltcl).sum();
    let stcl = (summary.stcl_carried_forward - st_left_before).max(Decimal::ZERO);
    let ltcl = (summary.ltcl_carried_forward - lt_left_before).max(Decimal::ZERO);
    losses.retain(|l| !l.stcl.is_zero() || !l.ltcl.is_zero());
    if !stcl.is_zero() || !ltcl.is_zero() {
        losses.push(LossYear { year, stcl, ltcl });
    }
}

/// Financial year `fy_start_year` (e.g. 2024 for FY 2024-25) runs from April 1 of `fy_start_year`
/// to March 31 of `fy_start_year + 1`.
pub async fn compute_indian_tax_summary(
    lots_repo: &dyn LotRepositoryTrait,
    asset_service: &dyn AssetServiceTrait,
    activity_service: &dyn ActivityServiceTrait,
    account_ids: &[String],
    fy_start_year: i32,
) -> Result<IndianTaxYearSummary> {
    let empty = || IndianTaxYearSummary {
        fy_start_year,
        ..Default::default()
    };
    if account_ids.is_empty() {
        return Ok(empty());
    }

    // Retrieve all lot open dates
    let all_lots = lots_repo.get_all_lots().await?;
    let mut lot_open_dates = HashMap::new();
    for lot in &all_lots {
        if let Ok(date) = NaiveDate::parse_from_str(&lot.open_date[..10], "%Y-%m-%d") {
            lot_open_dates.insert(lot.id.clone(), date);
        }
    }
    let lots_by_id: HashMap<&str, &LotRecord> =
        all_lots.iter().map(|lot| (lot.id.as_str(), lot)).collect();

    // Retrieve assets to build classification and grandfathering FMV maps
    let assets = asset_service.get_assets()?;
    let mut asset_classifications = HashMap::new();
    let mut asset_fmv_2018 = HashMap::new();
    let mut asset_info_map = HashMap::new();

    for asset in &assets {
        let classification = classify_asset_for_indian_tax(asset);
        asset_classifications.insert(asset.id.clone(), classification);
        if let Some(fmv) = extract_fmv_2018(asset) {
            asset_fmv_2018.insert(asset.id.clone(), fmv);
        }

        let isin = asset
            .metadata
            .as_ref()
            .and_then(|m| m.get("identifiers"))
            .and_then(|v| v.as_object())
            .and_then(|ids| ids.get("isin"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        asset_info_map.insert(
            asset.id.clone(),
            IndianTaxAssetInfo {
                name: asset.name.clone().unwrap_or_default(),
                symbol: asset
                    .display_code
                    .clone()
                    .or_else(|| asset.instrument_symbol.clone()),
                isin,
            },
        );
    }

    // Section 74: a loss is carried forward for eight years. Work through the
    // earlier years in order, so each year's unabsorbed loss reaches this one.
    let mut losses: Vec<LossYear> = Vec::new();
    let mut disposals = Vec::new();
    let mut summary = empty();
    for year in (fy_start_year - LOSS_CARRY_YEARS)..=fy_start_year {
        let (Some(start_date_exclusive), Some(end_date_inclusive)) = (
            NaiveDate::from_ymd_opt(year, 3, 31),
            NaiveDate::from_ymd_opt(year + 1, 3, 31),
        ) else {
            return Err(ValidationError::InvalidInput(format!(
                "Financial year {fy_start_year} is out of range"
            ))
            .into());
        };
        disposals = lots_repo
            .get_lot_disposals_for_accounts_in_date_range(
                account_ids,
                start_date_exclusive,
                end_date_inclusive,
            )
            .await?;

        let mut sell_trades: HashMap<String, Option<Activity>> = HashMap::new();
        for d in &disposals {
            if !sell_trades.contains_key(&d.disposal_activity_id) {
                let trade = activity_service.get_activity(&d.disposal_activity_id).ok();
                sell_trades.insert(d.disposal_activity_id.clone(), trade);
            }
        }
        let charges: HashMap<String, IndianDisposalCharges> = disposals
            .iter()
            .map(|d| {
                let lot = lots_by_id.get(d.lot_id.as_str()).copied();
                let trade = sell_trades
                    .get(&d.disposal_activity_id)
                    .and_then(Option::as_ref);
                (d.id.clone(), disposal_charges(d, lot, trade))
            })
            .collect();

        losses.retain(|l| l.year >= year - LOSS_CARRY_YEARS);
        let brought_forward = IndianLossesBroughtForward {
            stcl: losses.iter().map(|l| l.stcl).sum(),
            ltcl: losses.iter().map(|l| l.ltcl).sum(),
        };
        summary = calculate_indian_capital_gains(
            &disposals,
            &lot_open_dates,
            &asset_classifications,
            &asset_fmv_2018,
            &asset_info_map,
            &charges,
            brought_forward,
            year,
        );
        carry_losses_forward(&mut losses, &summary, year);
    }
    // What can still go to next year: drop the losses whose eighth year this is.
    losses.retain(|l| l.year > fy_start_year - LOSS_CARRY_YEARS);
    summary.stcl_carried_forward = losses.iter().map(|l| l.stcl).sum();
    summary.ltcl_carried_forward = losses.iter().map(|l| l.ltcl).sum();
    let other_sources = summarize_other_sources(
        &activity_service.get_activities_by_account_ids(account_ids)?,
        fy_start_year,
    );
    if disposals.is_empty()
        && summary.stcl_carried_forward.is_zero()
        && summary.ltcl_carried_forward.is_zero()
    {
        return Ok(IndianTaxYearSummary {
            other_sources,
            ..empty()
        });
    }
    summary.other_sources = other_sources;

    // Funds whose tax type was guessed: a debt or international fund guessed as
    // an equity fund is taxed at the wrong rate, so ask for it to be set.
    let sold: std::collections::HashSet<&str> =
        disposals.iter().map(|d| d.asset_id.as_str()).collect();
    let mut guessed: Vec<String> = assets
        .iter()
        .filter(|a| sold.contains(a.id.as_str()))
        .filter(|a| explicit_tax_classification(a).is_none())
        .filter_map(|a| {
            let kind = match classify_asset_for_indian_tax(a) {
                IndianAssetTaxClassification::EquityMf => "equity fund",
                IndianAssetTaxClassification::DebtMf => "debt fund",
                IndianAssetTaxClassification::NonEquityEtf => "gold or international ETF",
                IndianAssetTaxClassification::NonEquityFund => "gold or international fund",
                _ => return None,
            };
            let name = a.name.clone().unwrap_or_else(|| a.id.clone());
            Some(format!("{name} ({kind})"))
        })
        .collect();
    guessed.sort();
    if !guessed.is_empty() {
        summary.warnings.push(format!(
            "The tax type of these funds was guessed from their names: {}. If any is \
             wrong, set its Indian tax type on the asset's page.",
            guessed.join(", ")
        ));
    }

    let excluded = super::cleartax_excluded_count(&summary);
    if excluded > 0 {
        summary.warnings.push(format!(
            "{excluded} sale(s) of gold, bonds or other assets are not in the ClearTax file. \
             Enter them in ClearTax by hand."
        ));
    }
    summary.cleartax_files = super::cleartax_file_count(&summary);

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{AssetKind, QuoteMode};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn test_classify_asset_for_indian_tax() {
        let mut equity_asset = Asset {
            id: "asset-1".to_string(),
            kind: AssetKind::Investment,
            name: Some("Reliance Industries".to_string()),
            display_code: Some("RELIANCE".to_string()),
            notes: None,
            metadata: None,
            is_active: true,
            quote_mode: QuoteMode::Market,
            quote_ccy: "INR".to_string(),
            instrument_type: Some(InstrumentType::Equity),
            instrument_symbol: Some("RELIANCE".to_string()),
            instrument_exchange_mic: Some("XNSE".to_string()),
            instrument_key: None,
            provider_config: None,
            exchange_name: None,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        assert_eq!(
            classify_asset_for_indian_tax(&equity_asset),
            IndianAssetTaxClassification::EquityListed
        );

        equity_asset.instrument_exchange_mic = Some("AMFI".to_string());
        assert_eq!(
            classify_asset_for_indian_tax(&equity_asset),
            IndianAssetTaxClassification::EquityMf
        );

        equity_asset.name = Some("Embassy Office Parks REIT".to_string());
        assert_eq!(
            classify_asset_for_indian_tax(&equity_asset),
            IndianAssetTaxClassification::ReitInvIt
        );

        // Metadata override
        equity_asset.metadata = Some(serde_json::json!({
            "taxClassification": "debt_mf_post_2023"
        }));
        assert_eq!(
            classify_asset_for_indian_tax(&equity_asset),
            IndianAssetTaxClassification::DebtMf
        );
    }

    #[test]
    fn funds_are_guessed_from_their_names() {
        let guess = |name: &str, mic: &str| {
            let asset = Asset {
                name: Some(name.to_string()),
                instrument_type: Some(InstrumentType::Equity),
                instrument_exchange_mic: Some(mic.to_string()),
                ..Asset::default()
            };
            classify_asset_for_indian_tax(&asset)
        };
        use IndianAssetTaxClassification::*;
        assert_eq!(guess("Nippon India ETF Gold BeES", "XNSE"), NonEquityEtf);
        assert_eq!(guess("Motilal Oswal NASDAQ 100 ETF", "XNSE"), NonEquityEtf);
        assert_eq!(
            guess(
                "Motilal Oswal Nasdaq 100 Fund of Fund - Direct Plan Growth",
                "AMFI"
            ),
            NonEquityFund
        );
        assert_eq!(
            guess("Nippon India ETF Nifty 1D Rate Liquid BeES", "XNSE"),
            DebtMf
        );
        assert_eq!(guess("Bharat Bond ETF - April 2030", "XNSE"), DebtMf);
        assert_eq!(
            guess("Axis Short Term Fund - Direct Growth", "AMFI"),
            DebtMf
        );
        assert_eq!(guess("SBI Nifty 50 ETF", "XNSE"), EquityMf);
        assert_eq!(guess("Goldman Sachs India Equity Fund", "AMFI"), EquityMf);
        assert_eq!(
            guess("Capitalmind Flexi Cap Fund - Direct", "AMFI"),
            EquityMf
        );
        assert_eq!(guess("Sovereign Gold Bond 2.50% Aug 2028", "XNSE"), GoldSgb);
        assert_eq!(guess("Reliance Industries", "XNSE"), EquityListed);
    }

    #[test]
    fn test_extract_fmv_2018() {
        let mut asset = Asset::default();
        assert_eq!(extract_fmv_2018(&asset), None);

        asset.metadata = Some(serde_json::json!({
            "fmv20180131": "1250.50"
        }));
        assert_eq!(extract_fmv_2018(&asset), Some(dec!(1250.50)));
    }

    #[test]
    fn charges_are_shared_by_the_units_sold() {
        // 40 of a 100-unit trade's fee of 50.
        assert_eq!(share(dec!(-50), dec!(40), dec!(100)), dec!(20));
        // Never more than the whole charge, and nothing from a zero quantity.
        assert_eq!(share(dec!(50), dec!(120), dec!(100)), dec!(50));
        assert_eq!(share(dec!(50), dec!(1), Decimal::ZERO), Decimal::ZERO);
    }

    #[test]
    fn losses_are_used_oldest_first_and_new_ones_added() {
        let mut losses = vec![
            LossYear {
                year: 2017,
                stcl: dec!(10),
                ltcl: dec!(5),
            },
            LossYear {
                year: 2020,
                stcl: dec!(20),
                ltcl: Decimal::ZERO,
            },
        ];
        let summary = IndianTaxYearSummary {
            stcl_brought_forward_set_off: dec!(15),
            ltcl_brought_forward_set_off: dec!(5),
            // 15 of the old losses left plus 7 of this year's own.
            stcl_carried_forward: dec!(22),
            ltcl_carried_forward: dec!(3),
            ..Default::default()
        };
        carry_losses_forward(&mut losses, &summary, 2024);
        let left: Vec<_> = losses.iter().map(|l| (l.year, l.stcl, l.ltcl)).collect();
        assert_eq!(
            left,
            vec![(2020, dec!(15), dec!(0)), (2024, dec!(7), dec!(3))]
        );
    }
}
