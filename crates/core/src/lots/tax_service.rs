use std::collections::HashMap;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value;

use super::{
    calculate_indian_capital_gains, IndianAssetTaxClassification, IndianTaxYearSummary,
    LotRepositoryTrait,
};
use crate::assets::{Asset, AssetServiceTrait, InstrumentType};
use crate::errors::Result;

/// Classifies an asset for Indian capital gains taxation based on its instrument type,
/// exchange MIC, and explicit metadata overrides.
pub fn classify_asset_for_indian_tax(asset: &Asset) -> IndianAssetTaxClassification {
    // 1. Check explicit override in metadata
    if let Some(metadata) = &asset.metadata {
        if let Some(val) = metadata.get("taxClassification").or_else(|| metadata.get("assetTaxClassification")) {
            if let Some(s) = val.as_str() {
                match s.to_lowercase().as_str() {
                    "equity_listed" | "equitylisted" => return IndianAssetTaxClassification::EquityListed,
                    "equity_mf" | "equitymf" => return IndianAssetTaxClassification::EquityMf,
                    "reit_invit" | "reitinvit" => return IndianAssetTaxClassification::ReitInvIt,
                    "debt_mf_post_2023" | "debtmfpost2023" => return IndianAssetTaxClassification::DebtMfPost2023,
                    "unlisted_debenture_post_2024" | "unlisteddebenturepost2024" => {
                        return IndianAssetTaxClassification::UnlistedDebenturePost2024;
                    }
                    "other_capital_asset" | "othercapitalasset" => return IndianAssetTaxClassification::OtherCapitalAsset,
                    "gold_sgb" | "goldsgb" => return IndianAssetTaxClassification::GoldSgb,
                    _ => {}
                }
            }
        }
    }

    // 2. Derive classification from instrument identity & metadata
    match asset.instrument_type {
        Some(InstrumentType::Equity) => {
            let name_lower = asset.name.as_deref().unwrap_or("").to_lowercase();
            let symbol_lower = asset.instrument_symbol.as_deref().unwrap_or("").to_lowercase();
            let mic = asset.instrument_exchange_mic.as_deref().unwrap_or("");

            if name_lower.contains("reit")
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
            {
                IndianAssetTaxClassification::EquityMf
            } else {
                IndianAssetTaxClassification::EquityListed
            }
        }
        Some(InstrumentType::Metal) => IndianAssetTaxClassification::GoldSgb,
        Some(InstrumentType::Bond) => IndianAssetTaxClassification::OtherCapitalAsset,
        _ => IndianAssetTaxClassification::OtherCapitalAsset,
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
/// Financial year `fy_start_year` (e.g. 2024 for FY 2024-25) runs from April 1 of `fy_start_year`
/// to March 31 of `fy_start_year + 1`.
pub async fn compute_indian_tax_summary(
    lots_repo: &dyn LotRepositoryTrait,
    asset_service: &dyn AssetServiceTrait,
    account_ids: &[String],
    fy_start_year: i32,
) -> Result<IndianTaxYearSummary> {
    if account_ids.is_empty() {
        return Ok(IndianTaxYearSummary::default());
    }

    let start_date_exclusive = NaiveDate::from_ymd_opt(fy_start_year, 3, 31).unwrap();
    let end_date_inclusive = NaiveDate::from_ymd_opt(fy_start_year + 1, 3, 31).unwrap();

    let disposals = lots_repo
        .get_lot_disposals_for_accounts_in_date_range(
            account_ids,
            start_date_exclusive,
            end_date_inclusive,
        )
        .await?;

    if disposals.is_empty() {
        return Ok(IndianTaxYearSummary::default());
    }

    // Retrieve all lot open dates
    let all_lots = lots_repo.get_all_lots().await?;
    let mut lot_open_dates = HashMap::new();
    for lot in all_lots {
        if let Ok(date) = NaiveDate::parse_from_str(&lot.open_date[..10], "%Y-%m-%d") {
            lot_open_dates.insert(lot.id, date);
        }
    }

    // Retrieve assets to build classification and grandfathering FMV maps
    let assets = asset_service.get_assets()?;
    let mut asset_classifications = HashMap::new();
    let mut asset_fmv_2018 = HashMap::new();

    for asset in &assets {
        let classification = classify_asset_for_indian_tax(asset);
        asset_classifications.insert(asset.id.clone(), classification);
        if let Some(fmv) = extract_fmv_2018(asset) {
            asset_fmv_2018.insert(asset.id.clone(), fmv);
        }
    }

    let summary = calculate_indian_capital_gains(
        &disposals,
        &lot_open_dates,
        &asset_classifications,
        &asset_fmv_2018,
    );

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
            IndianAssetTaxClassification::DebtMfPost2023
        );
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
}
