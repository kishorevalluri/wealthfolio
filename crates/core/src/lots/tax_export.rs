//! Schedule CG / ITR export formatters for Indian capital gains.

use rust_decimal::Decimal;
use serde::Serialize;

use super::{HoldingPeriodCategory, IndianAssetTaxClassification, IndianTaxYearSummary};

/// A single row in the Schedule CG CSV export.
/// Column order matches the ITR-2/ITR-3 Schedule CG format.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCgRow {
    pub isin: String,
    pub asset_name: String,
    pub symbol: String,
    pub section: String,
    pub holding_period: String,
    pub acquisition_date: String,
    pub disposal_date: String,
    pub quantity: Decimal,
    pub sale_price_per_unit: Decimal,
    pub full_sale_consideration: Decimal,
    pub cost_of_acquisition: Decimal,
    pub fmv_per_unit_jan_31_2018: Option<Decimal>,
    pub deemed_cost_of_acquisition: Decimal,
    pub transfer_expenses: Decimal,
    pub total_deductions: Decimal,
    pub capital_gain: Decimal,
    pub applicable_tax_rate_pct: Decimal,
    pub estimated_tax: Decimal,
}

/// Determines the applicable ITR section string for a given classification and holding period.
fn section_label(
    classification: IndianAssetTaxClassification,
    holding_period: HoldingPeriodCategory,
) -> &'static str {
    match (classification, holding_period) {
        (
            IndianAssetTaxClassification::EquityListed
            | IndianAssetTaxClassification::EquityMf
            | IndianAssetTaxClassification::ReitInvIt,
            HoldingPeriodCategory::ShortTerm,
        ) => "Section 111A",
        (
            IndianAssetTaxClassification::EquityListed
            | IndianAssetTaxClassification::EquityMf
            | IndianAssetTaxClassification::ReitInvIt,
            HoldingPeriodCategory::LongTerm,
        ) => "Section 112A",
        (
            IndianAssetTaxClassification::OtherCapitalAsset
            | IndianAssetTaxClassification::GoldSgb,
            HoldingPeriodCategory::LongTerm,
        ) => "Section 112",
        (
            IndianAssetTaxClassification::DebtMfPost2023
            | IndianAssetTaxClassification::UnlistedDebenturePost2024,
            _,
        ) => "Section 50AA (Slab Rate)",
        (_, HoldingPeriodCategory::ShortTerm) => "Slab Rate",
    }
}

fn holding_period_label(hp: HoldingPeriodCategory) -> &'static str {
    match hp {
        HoldingPeriodCategory::ShortTerm => "Short Term",
        HoldingPeriodCategory::LongTerm => "Long Term",
    }
}

/// Converts an `IndianTaxYearSummary` into Schedule CG export rows.
pub fn to_schedule_cg_rows(summary: &IndianTaxYearSummary) -> Vec<ScheduleCgRow> {
    summary
        .calculations
        .iter()
        .map(|calc| {
            let total_deductions = calc.deemed_cost + calc.transfer_expenses;
            ScheduleCgRow {
                isin: calc.isin.clone().unwrap_or_default(),
                asset_name: calc.asset_name.clone(),
                symbol: calc.asset_symbol.clone().unwrap_or_default(),
                section: section_label(calc.classification, calc.holding_period).to_string(),
                holding_period: holding_period_label(calc.holding_period).to_string(),
                acquisition_date: calc.acquisition_date.format("%d/%m/%Y").to_string(),
                disposal_date: calc.disposal_date.format("%d/%m/%Y").to_string(),
                quantity: calc.quantity,
                sale_price_per_unit: calc.sale_price_per_unit,
                full_sale_consideration: calc.proceeds,
                cost_of_acquisition: calc.actual_cost,
                fmv_per_unit_jan_31_2018: calc.fmv_per_unit_2018,
                deemed_cost_of_acquisition: calc.deemed_cost,
                transfer_expenses: calc.transfer_expenses,
                total_deductions,
                capital_gain: calc.gross_gain,
                applicable_tax_rate_pct: calc.applicable_tax_rate * Decimal::ONE_HUNDRED,
                estimated_tax: calc.estimated_tax,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lots::IndianCapitalGainCalculation;
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    #[test]
    fn section_labels_correct() {
        assert_eq!(
            section_label(
                IndianAssetTaxClassification::EquityListed,
                HoldingPeriodCategory::ShortTerm
            ),
            "Section 111A"
        );
        assert_eq!(
            section_label(
                IndianAssetTaxClassification::EquityMf,
                HoldingPeriodCategory::LongTerm
            ),
            "Section 112A"
        );
        assert_eq!(
            section_label(
                IndianAssetTaxClassification::DebtMfPost2023,
                HoldingPeriodCategory::ShortTerm
            ),
            "Section 50AA (Slab Rate)"
        );
        assert_eq!(
            section_label(
                IndianAssetTaxClassification::GoldSgb,
                HoldingPeriodCategory::LongTerm
            ),
            "Section 112"
        );
    }

    #[test]
    fn to_schedule_cg_rows_produces_correct_output() {
        let summary = IndianTaxYearSummary {
            calculations: vec![IndianCapitalGainCalculation {
                disposal_id: "d1".to_string(),
                lot_id: "l1".to_string(),
                asset_id: "a1".to_string(),
                classification: IndianAssetTaxClassification::EquityListed,
                acquisition_date: NaiveDate::from_ymd_opt(2017, 6, 15).unwrap(),
                disposal_date: NaiveDate::from_ymd_opt(2024, 8, 10).unwrap(),
                holding_period: HoldingPeriodCategory::LongTerm,
                actual_cost: dec!(100000),
                deemed_cost: dec!(150000),
                proceeds: dec!(200000),
                gross_gain: dec!(50000),
                taxable_gain: dec!(50000),
                applicable_tax_rate: dec!(0.125),
                estimated_tax: dec!(6250),
                asset_name: "Reliance Industries".to_string(),
                asset_symbol: Some("RELIANCE".to_string()),
                isin: Some("INE002A01018".to_string()),
                quantity: dec!(100),
                sale_price_per_unit: dec!(2000),
                cost_per_unit: dec!(1000),
                fmv_per_unit_2018: Some(dec!(1500)),
                transfer_expenses: dec!(0),
            }],
            ..Default::default()
        };

        let rows = to_schedule_cg_rows(&summary);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.section, "Section 112A");
        assert_eq!(row.isin, "INE002A01018");
        assert_eq!(row.acquisition_date, "15/06/2017");
        assert_eq!(row.disposal_date, "10/08/2024");
        assert_eq!(row.quantity, dec!(100));
        assert_eq!(row.full_sale_consideration, dec!(200000));
        assert_eq!(row.deemed_cost_of_acquisition, dec!(150000));
        assert_eq!(row.capital_gain, dec!(50000));
        assert_eq!(row.applicable_tax_rate_pct, dec!(12.5));
    }
}
