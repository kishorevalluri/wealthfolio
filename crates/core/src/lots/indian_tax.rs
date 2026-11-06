//! Indian capital gains overlay on lot disposals.
//!
//! Classifies disposals into STCG/LTCG buckets (Sections 111A, 112A, 112 and
//! 50AA), applies Section 55(2)(ac) grandfathering, the Budget 2024 rate split
//! and the Section 112A exemption, and nets losses under Section 70.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::LotDisposal;


/// Classification of Indian capital assets for holding period and tax rate determination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndianAssetTaxClassification {
    /// Listed Indian equity shares and equity-oriented mutual funds (>65% domestic equity).
    /// Holding period: 12 months for LTCG.
    EquityListed,
    /// Equity-oriented mutual funds.
    EquityMf,
    /// Units of listed business trusts (REITs & InvITs). Holding period: 12 months post Budget 2024.
    ReitInvIt,
    /// Specified mutual funds under Section 50AA (acquired on or after April 1, 2023 with <=35% equity).
    /// Deemed short-term capital assets regardless of holding duration.
    DebtMfPost2023,
    /// Unlisted bonds and debentures transferred on or after July 23, 2024 (Section 50AA Finance Act 2024).
    /// Deemed short-term capital assets regardless of holding duration.
    UnlistedDebenturePost2024,
    /// Sovereign Gold Bonds, unlisted equity, debt MFs pre-April 2023, real estate, etc.
    OtherCapitalAsset,
    /// Gold or Sovereign Gold Bonds.
    GoldSgb,
}

/// Character of capital gain based on asset classification and holding period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldingPeriodCategory {
    ShortTerm,
    LongTerm,
}

/// Calculates deemed cost of acquisition under Section 55(2)(ac) (Grandfathering for listed equity acquired on or before 31-Jan-2018).
///
/// Formula:
/// Deemed Cost = max(Actual Cost, min(FMV on 31-Jan-2018, Sale Price))
///
/// If acquisition date is after 2018-01-31 or FMV is None, returns `actual_cost`.
pub fn calculate_grandfathered_cost_basis(
    acquisition_date: NaiveDate,
    actual_cost: Decimal,
    fmv_2018_01_31: Option<Decimal>,
    sale_price: Decimal,
) -> Decimal {
    let cutoff = NaiveDate::from_ymd_opt(2018, 1, 31).unwrap();
    if acquisition_date > cutoff {
        return actual_cost;
    }

    match fmv_2018_01_31 {
        Some(fmv) => {
            let lower_of_fmv_and_sale = fmv.min(sale_price);
            actual_cost.max(lower_of_fmv_and_sale)
        }
        None => actual_cost,
    }
}

/// Returns statutory Indian equity tax rates and Section 112A threshold for a given disposal date.
///
/// Returns: (stcg_rate, ltcg_rate, sec_112a_exemption_limit)
///
/// Boundary:
/// - Disposals before 23-Jul-2024 (Budget 2024): STCG 15% (0.15), LTCG 10% (0.10), Sec 112A ₹1,00,000 (100000).
/// - Disposals on or after 23-Jul-2024: STCG 20% (0.20), LTCG 12.5% (0.125), Sec 112A ₹1,25,000 (125000).
pub fn indian_equity_tax_rates_for_date(disposal_date: NaiveDate) -> (Decimal, Decimal, Decimal) {
    let budget_2024_date = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();
    if disposal_date < budget_2024_date {
        (
            Decimal::new(15, 2),
            Decimal::new(10, 2),
            Decimal::new(100000, 0),
        )
    } else {
        (
            Decimal::new(20, 2),
            Decimal::new(125, 3), // 0.125
            Decimal::new(125000, 0),
        )
    }
}

/// Determines whether a capital gain is Short-Term or Long-Term under Indian Income Tax provisions.
pub fn determine_holding_period_category(
    acquisition_date: NaiveDate,
    disposal_date: NaiveDate,
    classification: IndianAssetTaxClassification,
) -> HoldingPeriodCategory {
    // Section 50AA: specified debt mutual funds and unlisted debentures are deemed short-term capital assets
    if classification == IndianAssetTaxClassification::DebtMfPost2023
        || classification == IndianAssetTaxClassification::UnlistedDebenturePost2024
    {
        return HoldingPeriodCategory::ShortTerm;
    }

    // Days difference calculation
    let days = (disposal_date - acquisition_date).num_days();
    let budget_2024_date = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();

    match classification {
        IndianAssetTaxClassification::EquityListed | IndianAssetTaxClassification::EquityMf => {
            // Listed equity & equity mutual funds: > 12 months (365 days) is Long Term
            if days > 365 {
                HoldingPeriodCategory::LongTerm
            } else {
                HoldingPeriodCategory::ShortTerm
            }
        }
        IndianAssetTaxClassification::ReitInvIt => {
            // Pre-Budget 2024: 36 months (1095 days). Post-Budget 2024: 12 months (365 days).
            let threshold = if disposal_date < budget_2024_date { 1095 } else { 365 };
            if days > threshold {
                HoldingPeriodCategory::LongTerm
            } else {
                HoldingPeriodCategory::ShortTerm
            }
        }
        IndianAssetTaxClassification::OtherCapitalAsset | IndianAssetTaxClassification::GoldSgb => {
            // Pre-Budget 2024: 36 months (1095 days). Post-Budget 2024: 24 months (730 days).
            let threshold = if disposal_date < budget_2024_date { 1095 } else { 730 };
            if days > threshold {
                HoldingPeriodCategory::LongTerm
            } else {
                HoldingPeriodCategory::ShortTerm
            }
        }
        IndianAssetTaxClassification::DebtMfPost2023
        | IndianAssetTaxClassification::UnlistedDebenturePost2024 => {
            HoldingPeriodCategory::ShortTerm
        }
    }
}

/// Individual lot disposal tax calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianCapitalGainCalculation {
    pub disposal_id: String,
    pub lot_id: String,
    pub asset_id: String,
    pub classification: IndianAssetTaxClassification,
    pub acquisition_date: NaiveDate,
    pub disposal_date: NaiveDate,
    pub holding_period: HoldingPeriodCategory,
    pub actual_cost: Decimal,
    pub deemed_cost: Decimal,
    pub proceeds: Decimal,
    pub gross_gain: Decimal,
    pub taxable_gain: Decimal,
    pub applicable_tax_rate: Decimal,
    pub estimated_tax: Decimal,
    // ── Fields for Schedule CG / ITR export ──
    pub asset_name: String,
    pub asset_symbol: Option<String>,
    pub isin: Option<String>,
    pub quantity: Decimal,
    pub sale_price_per_unit: Decimal,
    pub cost_per_unit: Decimal,
    pub fmv_per_unit_2018: Option<Decimal>,
    pub transfer_expenses: Decimal,
}

/// Compact asset info used by the Indian tax calculator for export fields.
pub struct IndianTaxAssetInfo {
    pub name: String,
    pub symbol: Option<String>,
    pub isin: Option<String>,
}

/// Annual Indian Tax Summary with Section 112A exemption pooling, loss set-offs, and pre/post Budget 2024 pools
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndianTaxYearSummary {
    /// Pre-July 23, 2024 Section 111A STCG (@ 15%)
    pub stcg_111a_pre_budget: Decimal,
    /// Post-July 23, 2024 Section 111A STCG (@ 20%)
    pub stcg_111a_post_budget: Decimal,
    /// STCG taxable at individual slab rates (Section 50AA, unlisted securities, etc.)
    pub stcg_slab_total: Decimal,
    /// Total STCG before loss set-off
    pub stcg_total: Decimal,
    /// Pre-July 23, 2024 Section 112A LTCG (@ 10%)
    pub ltcg_112a_pre_budget: Decimal,
    /// Post-July 23, 2024 Section 112A LTCG (@ 12.5%)
    pub ltcg_112a_post_budget: Decimal,
    /// Total gross Section 112A LTCG
    pub sec_112a_ltcg_gross: Decimal,
    /// Section 112A exemption applied
    pub sec_112a_exemption_applied: Decimal,
    /// Net taxable Section 112A LTCG after exemption
    pub sec_112a_ltcg_taxable: Decimal,
    /// Other LTCG taxable under Section 112 (12.5% / 20%)
    pub ltcg_other_total: Decimal,
    /// Total LTCG before loss set-off
    pub ltcg_total: Decimal,
    /// Unabsorbed short-term capital loss to carry forward
    pub stcl_carried_forward: Decimal,
    /// Unabsorbed long-term capital loss to carry forward
    pub ltcl_carried_forward: Decimal,
    /// Estimated tax on short-term gains (special rate buckets)
    pub estimated_stcg_tax: Decimal,
    /// Estimated tax on long-term gains
    pub estimated_ltcg_tax: Decimal,
    pub calculations: Vec<IndianCapitalGainCalculation>,
}

/// Calculates Indian capital gains for a series of lot disposals, handling grandfathering under
/// Section 55(2)(ac), Budget 2024 rate cut-offs, Section 112A exemption pooling, and statutory loss set-offs (Sec 70/71).
pub fn calculate_indian_capital_gains(
    disposals: &[LotDisposal],
    lot_open_dates: &std::collections::HashMap<String, NaiveDate>,
    asset_classifications: &std::collections::HashMap<String, IndianAssetTaxClassification>,
    asset_fmv_2018: &std::collections::HashMap<String, Decimal>,
    asset_info: &std::collections::HashMap<String, IndianTaxAssetInfo>,
) -> IndianTaxYearSummary {
    let mut summary = IndianTaxYearSummary::default();
    let budget_2024_date = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();

    // The ₹1.25L exemption under Section 112A applies annually across FY 2024-25 onwards.
    // Pre-FY24-25 defaults to ₹1,00,000 if all trades are strictly prior to 2024-04-01.
    let max_disposal_date = disposals
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(&d.disposal_date[..10], "%Y-%m-%d").ok())
        .max()
        .unwrap_or(budget_2024_date);

    let sec_112a_exemption_limit = if max_disposal_date >= NaiveDate::from_ymd_opt(2024, 4, 1).unwrap() {
        Decimal::new(125000, 0)
    } else {
        Decimal::new(100000, 0)
    };

    let mut ltcg_other_pre_pool = Decimal::ZERO;
    let mut ltcg_other_post_pool = Decimal::ZERO;

    for d in disposals {
        let disp_date = match NaiveDate::parse_from_str(&d.disposal_date[..10], "%Y-%m-%d") {
            Ok(dt) => dt,
            Err(_) => continue,
        };
        let acq_date = lot_open_dates
            .get(&d.lot_id)
            .copied()
            .unwrap_or(disp_date);

        let classification = asset_classifications
            .get(&d.asset_id)
            .copied()
            .unwrap_or(IndianAssetTaxClassification::OtherCapitalAsset);

        let cost_basis: Decimal = d.cost_basis_base.parse().unwrap_or(Decimal::ZERO);
        let proceeds: Decimal = d.proceeds_base.parse().unwrap_or(Decimal::ZERO);

        let fmv_2018 = asset_fmv_2018.get(&d.asset_id).copied();

        let info = asset_info.get(&d.asset_id);
        let asset_name = info.map(|i| i.name.clone()).unwrap_or_default();
        let asset_symbol = info.and_then(|i| i.symbol.clone());
        let isin = info.and_then(|i| i.isin.clone());

        let qty: Decimal = d.quantity.parse().unwrap_or(Decimal::ONE);
        let sale_price_per_unit = if qty > Decimal::ZERO { proceeds / qty } else { Decimal::ZERO };
        let cost_per_unit = if qty > Decimal::ZERO { cost_basis / qty } else { Decimal::ZERO };
        let fmv_per_unit_2018 = fmv_2018;
        let transfer_expenses = Decimal::ZERO;

        // Calculate grandfathered cost basis if applicable under Section 55(2)(ac)
        let deemed_cost = if classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt
        {
            calculate_grandfathered_cost_basis(acq_date, cost_basis, fmv_2018, proceeds)
        } else {
            cost_basis
        };

        let gross_gain = proceeds - deemed_cost;
        let holding_period = determine_holding_period_category(acq_date, disp_date, classification);
        let (stcg_rate, ltcg_rate, _) = indian_equity_tax_rates_for_date(disp_date);

        let is_sec_112a_eligible = (classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt)
            && holding_period == HoldingPeriodCategory::LongTerm;

        let is_sec_111a_eligible = (classification == IndianAssetTaxClassification::EquityListed
            || classification == IndianAssetTaxClassification::EquityMf
            || classification == IndianAssetTaxClassification::ReitInvIt)
            && holding_period == HoldingPeriodCategory::ShortTerm;

        let applicable_rate = match holding_period {
            HoldingPeriodCategory::ShortTerm => stcg_rate,
            HoldingPeriodCategory::LongTerm => ltcg_rate,
        };

        if is_sec_112a_eligible {
            if disp_date < budget_2024_date {
                summary.ltcg_112a_pre_budget += gross_gain;
            } else {
                summary.ltcg_112a_post_budget += gross_gain;
            }
            summary.sec_112a_ltcg_gross += gross_gain;
        } else if holding_period == HoldingPeriodCategory::LongTerm {
            if disp_date < budget_2024_date {
                ltcg_other_pre_pool += gross_gain;
            } else {
                ltcg_other_post_pool += gross_gain;
            }
            summary.ltcg_other_total += gross_gain;
        }

        match holding_period {
            HoldingPeriodCategory::ShortTerm => {
                summary.stcg_total += gross_gain;
                if is_sec_111a_eligible {
                    if disp_date < budget_2024_date {
                        summary.stcg_111a_pre_budget += gross_gain;
                    } else {
                        summary.stcg_111a_post_budget += gross_gain;
                    }
                } else {
                    summary.stcg_slab_total += gross_gain;
                }
            }
            HoldingPeriodCategory::LongTerm => {
                summary.ltcg_total += gross_gain;
            }
        }

        summary.calculations.push(IndianCapitalGainCalculation {
            disposal_id: d.id.clone(),
            lot_id: d.lot_id.clone(),
            asset_id: d.asset_id.clone(),
            classification,
            acquisition_date: acq_date,
            disposal_date: disp_date,
            holding_period,
            actual_cost: cost_basis,
            deemed_cost,
            proceeds,
            gross_gain,
            taxable_gain: gross_gain,
            applicable_tax_rate: applicable_rate,
            estimated_tax: Decimal::ZERO,
            // Schedule CG export fields
            asset_name,
            asset_symbol,
            isin,
            quantity: qty,
            sale_price_per_unit,
            cost_per_unit,
            fmv_per_unit_2018,
            transfer_expenses,
        });
    }

    // ── Section 70 Intra-Head Loss Set-Off & Section 112A Exemption ──

    // 1. Netting Section 112A LTCG:
    // If one pool (pre vs post) is positive and the other is negative, net them.
    let mut net_112a_pre = summary.ltcg_112a_pre_budget;
    let mut net_112a_post = summary.ltcg_112a_post_budget;

    if net_112a_pre < Decimal::ZERO && net_112a_post > Decimal::ZERO {
        let abs_loss = net_112a_pre.abs();
        if net_112a_post >= abs_loss {
            net_112a_post -= abs_loss;
            net_112a_pre = Decimal::ZERO;
        } else {
            net_112a_pre += net_112a_post;
            net_112a_post = Decimal::ZERO;
        }
    } else if net_112a_post < Decimal::ZERO && net_112a_pre > Decimal::ZERO {
        let abs_loss = net_112a_post.abs();
        if net_112a_pre >= abs_loss {
            net_112a_pre -= abs_loss;
            net_112a_post = Decimal::ZERO;
        } else {
            net_112a_post += net_112a_pre;
            net_112a_pre = Decimal::ZERO;
        }
    }

    let positive_112a_total = net_112a_pre.max(Decimal::ZERO) + net_112a_post.max(Decimal::ZERO);
    let exemption_applied = positive_112a_total.min(sec_112a_exemption_limit);
    summary.sec_112a_exemption_applied = exemption_applied;
    summary.sec_112a_ltcg_taxable = (positive_112a_total - exemption_applied).max(Decimal::ZERO);

    // Apply exemption pre-budget first (10% bucket), then post-budget (12.5% bucket)
    let mut rem_exemption = exemption_applied;
    let taxable_112a_pre = if net_112a_pre > Decimal::ZERO {
        if net_112a_pre >= rem_exemption {
            let t = net_112a_pre - rem_exemption;
            rem_exemption = Decimal::ZERO;
            t
        } else {
            rem_exemption -= net_112a_pre;
            Decimal::ZERO
        }
    } else {
        Decimal::ZERO
    };
    let taxable_112a_post = if net_112a_post > Decimal::ZERO {
        (net_112a_post - rem_exemption).max(Decimal::ZERO)
    } else {
        Decimal::ZERO
    };

    // 2. Netting Section 111A STCG:
    let mut net_111a_pre = summary.stcg_111a_pre_budget;
    let mut net_111a_post = summary.stcg_111a_post_budget;
    let mut net_stcg_slab = summary.stcg_slab_total;

    // Cross-net STCL within STCG buckets
    if net_111a_pre < Decimal::ZERO {
        let abs_loss = net_111a_pre.abs();
        if net_111a_post >= abs_loss {
            net_111a_post -= abs_loss;
            net_111a_pre = Decimal::ZERO;
        } else {
            let absorbed = net_111a_post;
            net_111a_post = Decimal::ZERO;
            let rem_loss = abs_loss - absorbed;
            if net_stcg_slab >= rem_loss {
                net_stcg_slab -= rem_loss;
                net_111a_pre = Decimal::ZERO;
            } else {
                net_111a_pre = -(rem_loss - net_stcg_slab.max(Decimal::ZERO));
                net_stcg_slab = Decimal::ZERO;
            }
        }
    }

    if net_111a_post < Decimal::ZERO {
        let abs_loss = net_111a_post.abs();
        if net_111a_pre >= abs_loss {
            net_111a_pre -= abs_loss;
            net_111a_post = Decimal::ZERO;
        } else {
            let absorbed = net_111a_pre;
            net_111a_pre = Decimal::ZERO;
            let rem_loss = abs_loss - absorbed;
            if net_stcg_slab >= rem_loss {
                net_stcg_slab -= rem_loss;
                net_111a_post = Decimal::ZERO;
            } else {
                net_111a_post = -(rem_loss - net_stcg_slab.max(Decimal::ZERO));
                net_stcg_slab = Decimal::ZERO;
            }
        }
    }

    if net_stcg_slab < Decimal::ZERO {
        let mut abs_loss = net_stcg_slab.abs();
        if net_111a_post >= abs_loss {
            net_111a_post -= abs_loss;
            net_stcg_slab = Decimal::ZERO;
        } else {
            abs_loss -= net_111a_post;
            net_111a_post = Decimal::ZERO;
            if net_111a_pre >= abs_loss {
                net_111a_pre -= abs_loss;
                net_stcg_slab = Decimal::ZERO;
            } else {
                net_stcg_slab = -(abs_loss - net_111a_pre);
                net_111a_pre = Decimal::ZERO;
            }
        }
    }

    // 3. Section 70(2): Short-Term Capital Loss can be set off against Long-Term Capital Gains
    let mut unabsorbed_stcl = Decimal::ZERO;
    if net_111a_pre < Decimal::ZERO {
        unabsorbed_stcl += net_111a_pre.abs();
        net_111a_pre = Decimal::ZERO;
    }
    if net_111a_post < Decimal::ZERO {
        unabsorbed_stcl += net_111a_post.abs();
        net_111a_post = Decimal::ZERO;
    }
    if net_stcg_slab < Decimal::ZERO {
        unabsorbed_stcl += net_stcg_slab.abs();
    }

    // Offset residual STCL against Section 112 Other LTCG, then Section 112A taxable LTCG
    let mut taxable_other_pre = ltcg_other_pre_pool.max(Decimal::ZERO);
    let mut taxable_other_post = ltcg_other_post_pool.max(Decimal::ZERO);
    let mut final_taxable_112a_pre = taxable_112a_pre;
    let mut final_taxable_112a_post = taxable_112a_post;

    if unabsorbed_stcl > Decimal::ZERO && taxable_other_post > Decimal::ZERO {
        let absorb = unabsorbed_stcl.min(taxable_other_post);
        taxable_other_post -= absorb;
        unabsorbed_stcl -= absorb;
    }
    if unabsorbed_stcl > Decimal::ZERO && taxable_other_pre > Decimal::ZERO {
        let absorb = unabsorbed_stcl.min(taxable_other_pre);
        taxable_other_pre -= absorb;
        unabsorbed_stcl -= absorb;
    }
    if unabsorbed_stcl > Decimal::ZERO && final_taxable_112a_post > Decimal::ZERO {
        let absorb = unabsorbed_stcl.min(final_taxable_112a_post);
        final_taxable_112a_post -= absorb;
        unabsorbed_stcl -= absorb;
    }
    if unabsorbed_stcl > Decimal::ZERO && final_taxable_112a_pre > Decimal::ZERO {
        let absorb = unabsorbed_stcl.min(final_taxable_112a_pre);
        final_taxable_112a_pre -= absorb;
        unabsorbed_stcl -= absorb;
    }

    // 4. Compute exact estimated tax across net pools
    summary.estimated_stcg_tax = (net_111a_pre.max(Decimal::ZERO) * Decimal::new(15, 2))
        + (net_111a_post.max(Decimal::ZERO) * Decimal::new(20, 2));

    summary.estimated_ltcg_tax = (final_taxable_112a_pre * Decimal::new(10, 2))
        + (final_taxable_112a_post * Decimal::new(125, 3))
        + (taxable_other_pre * Decimal::new(20, 2))
        + (taxable_other_post * Decimal::new(125, 3));

    // 5. Section 74 Loss Carry-Forward
    summary.stcl_carried_forward = unabsorbed_stcl;
    if summary.ltcg_total < Decimal::ZERO {
        summary.ltcl_carried_forward = summary.ltcg_total.abs();
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;


    #[test]
    fn test_budget_2024_tax_rates_and_limits() {
        let pre_budget = NaiveDate::from_ymd_opt(2024, 7, 22).unwrap();
        let post_budget = NaiveDate::from_ymd_opt(2024, 7, 23).unwrap();

        let (stcg_pre, ltcg_pre, limit_pre) = indian_equity_tax_rates_for_date(pre_budget);
        assert_eq!(stcg_pre, dec!(0.15));
        assert_eq!(ltcg_pre, dec!(0.10));
        assert_eq!(limit_pre, dec!(100000));

        let (stcg_post, ltcg_post, limit_post) = indian_equity_tax_rates_for_date(post_budget);
        assert_eq!(stcg_post, dec!(0.20));
        assert_eq!(ltcg_post, dec!(0.125));
        assert_eq!(limit_post, dec!(125000));
    }

    #[test]
    fn test_section_55_2_ac_grandfathering_cases() {
        let pre_2018 = NaiveDate::from_ymd_opt(2017, 5, 10).unwrap();
        let post_2018 = NaiveDate::from_ymd_opt(2018, 2, 1).unwrap();

        // Case 1: Acquired after Jan 31, 2018 -> grandfathering does not apply
        let basis1 = calculate_grandfathered_cost_basis(post_2018, dec!(100), Some(dec!(150)), dec!(200));
        assert_eq!(basis1, dec!(100));

        // Case 2: Actual cost (100) < FMV (150) < Sale price (200)
        // Lower of (FMV, Sale) = 150. Max(100, 150) = 150. Gain taxed = 200 - 150 = 50.
        let basis2 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(200));
        assert_eq!(basis2, dec!(150));

        // Case 3: Actual cost (100) < Sale price (120) < FMV (150)
        // Lower of (FMV, Sale) = 120. Max(100, 120) = 120. Gain taxed = 120 - 120 = 0 (No artificial loss).
        let basis3 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(120));
        assert_eq!(basis3, dec!(120));

        // Case 4: Sale price (80) < FMV (150), Actual cost = 100
        // Lower of (FMV, Sale) = 80. Max(100, 80) = 100. Loss = 80 - 100 = -20.
        let basis4 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(150)), dec!(80));
        assert_eq!(basis4, dec!(100));

        // Case 5: FMV (80) < Actual cost (100) < Sale price (200)
        // Lower of (FMV, Sale) = 80. Max(100, 80) = 100. Basis remains 100.
        let basis5 = calculate_grandfathered_cost_basis(pre_2018, dec!(100), Some(dec!(80)), dec!(200));
        assert_eq!(basis5, dec!(100));
    }

    #[test]
    fn test_section_50aa_debt_mf_always_short_term() {
        let acq = NaiveDate::from_ymd_opt(2023, 4, 15).unwrap();
        let disp = NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(); // > 2 years holding

        // Standard equity would be LongTerm
        let equity_cat = determine_holding_period_category(acq, disp, IndianAssetTaxClassification::EquityListed);
        assert_eq!(equity_cat, HoldingPeriodCategory::LongTerm);

        // Section 50AA Debt MF is deemed ShortTerm regardless of duration
        let debt_cat = determine_holding_period_category(acq, disp, IndianAssetTaxClassification::DebtMfPost2023);
        assert_eq!(debt_cat, HoldingPeriodCategory::ShortTerm);
    }

    #[test]
    fn test_calculate_indian_capital_gains_and_112a_pooling() {
        // Lot 1: Pre-2018 equity sold post Budget 2024
        // Acq: 2017-01-01, Disp: 2024-08-01 (>12m -> LTCG)
        // Actual cost = 100,000, FMV 2018 = 150,000, Sale = 300,000
        // Grandfathered deemed cost = 150,000 -> LTCG = 150,000
        let disp1 = LotDisposal {
            id: "disp-1".to_string(),
            lot_id: "lot-1".to_string(),
            account_id: "acc-1".to_string(),
            asset_id: "RELIANCE".to_string(),
            disposal_activity_id: "act-1".to_string(),
            disposal_date: "2024-08-01T00:00:00.000Z".to_string(),
            quantity: "100".to_string(),
            proceeds: "300000".to_string(),
            cost_basis: "100000".to_string(),
            realized_pnl: "200000".to_string(),
            proceeds_base: "300000".to_string(),
            cost_basis_base: "100000".to_string(),
            realized_pnl_base: "200000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-01T00:00:00.000Z".to_string(),
        };

        // Lot 2: Equity sold within 6 months post Budget 2024 (STCG)
        // Cost = 50,000, Sale = 70,000 -> Gain = 20,000 @ 20%
        let disp2 = LotDisposal {
            id: "disp-2".to_string(),
            lot_id: "lot-2".to_string(),
            account_id: "acc-1".to_string(),
            asset_id: "TCS".to_string(),
            disposal_activity_id: "act-2".to_string(),
            disposal_date: "2024-08-05T00:00:00.000Z".to_string(),
            quantity: "10".to_string(),
            proceeds: "70000".to_string(),
            cost_basis: "50000".to_string(),
            realized_pnl: "20000".to_string(),
            proceeds_base: "70000".to_string(),
            cost_basis_base: "50000".to_string(),
            realized_pnl_base: "20000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-05T00:00:00.000Z".to_string(),
        };

        let mut lot_open_dates = HashMap::new();
        lot_open_dates.insert("lot-1".to_string(), NaiveDate::from_ymd_opt(2017, 1, 1).unwrap());
        lot_open_dates.insert("lot-2".to_string(), NaiveDate::from_ymd_opt(2024, 3, 1).unwrap());

        let mut asset_classifications = HashMap::new();
        asset_classifications.insert("RELIANCE".to_string(), IndianAssetTaxClassification::EquityListed);
        asset_classifications.insert("TCS".to_string(), IndianAssetTaxClassification::EquityListed);

        let mut asset_fmv_2018 = HashMap::new();
        asset_fmv_2018.insert("RELIANCE".to_string(), dec!(150000));

        let summary = calculate_indian_capital_gains(
            &[disp1, disp2],
            &lot_open_dates,
            &asset_classifications,
            &asset_fmv_2018,
            &HashMap::new(),
        );

        // STCG should be 20,000, tax at 20% = 4,000
        assert_eq!(summary.stcg_total, dec!(20000));
        assert_eq!(summary.estimated_stcg_tax, dec!(4000));

        // LTCG gross = 150,000 (after 150,000 grandfathering deemed cost)
        assert_eq!(summary.sec_112a_ltcg_gross, dec!(150000));
        // Sec 112A exemption = 125,000
        assert_eq!(summary.sec_112a_exemption_applied, dec!(125000));
        // Taxable LTCG = 150,000 - 125,000 = 25,000
        assert_eq!(summary.sec_112a_ltcg_taxable, dec!(25000));
        // LTCG tax at 12.5% on 25,000 = 3,125
        assert_eq!(summary.estimated_ltcg_tax, dec!(3125));
    }

    #[test]
    fn test_section_70_loss_set_off() {
        // Test that losing trades net against winning trades in same bucket,
        // and that STCL can offset LTCG under Section 70(2).
        let disp_stcg_gain = LotDisposal {
            id: "d1".to_string(),
            lot_id: "l1".to_string(),
            account_id: "a1".to_string(),
            asset_id: "INFY".to_string(),
            disposal_activity_id: "act1".to_string(),
            disposal_date: "2024-08-10T00:00:00.000Z".to_string(),
            quantity: "10".to_string(),
            proceeds: "100000".to_string(),
            cost_basis: "60000".to_string(),
            realized_pnl: "40000".to_string(),
            proceeds_base: "100000".to_string(),
            cost_basis_base: "60000".to_string(),
            realized_pnl_base: "40000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-10T00:00:00.000Z".to_string(),
        };

        let disp_stcg_loss = LotDisposal {
            id: "d2".to_string(),
            lot_id: "l2".to_string(),
            account_id: "a1".to_string(),
            asset_id: "WIPRO".to_string(),
            disposal_activity_id: "act2".to_string(),
            disposal_date: "2024-08-12T00:00:00.000Z".to_string(),
            quantity: "10".to_string(),
            proceeds: "40000".to_string(),
            cost_basis: "50000".to_string(),
            realized_pnl: "-10000".to_string(),
            proceeds_base: "40000".to_string(),
            cost_basis_base: "50000".to_string(),
            realized_pnl_base: "-10000".to_string(),
            currency: "INR".to_string(),
            base_currency: "INR".to_string(),
            fx_rate_to_base: "1".to_string(),
            cost_basis_method: "FIFO".to_string(),
            created_at: "2024-08-12T00:00:00.000Z".to_string(),
        };

        let mut lot_open_dates = HashMap::new();
        lot_open_dates.insert("l1".to_string(), NaiveDate::from_ymd_opt(2024, 7, 1).unwrap());
        lot_open_dates.insert("l2".to_string(), NaiveDate::from_ymd_opt(2024, 7, 1).unwrap());

        let mut asset_classifications = HashMap::new();
        asset_classifications.insert("INFY".to_string(), IndianAssetTaxClassification::EquityListed);
        asset_classifications.insert("WIPRO".to_string(), IndianAssetTaxClassification::EquityListed);

        let summary = calculate_indian_capital_gains(
            &[disp_stcg_gain, disp_stcg_loss],
            &lot_open_dates,
            &asset_classifications,
            &HashMap::new(),
            &HashMap::new(),
        );

        // Net STCG is 40,000 - 10,000 = 30,000.
        // Tax at 20% on net 30,000 = 6,000 (NOT 8,000 on gross 40,000!).
        assert_eq!(summary.stcg_total, dec!(30000));
        assert_eq!(summary.estimated_stcg_tax, dec!(6000));
        assert_eq!(summary.stcl_carried_forward, dec!(0));
    }

    #[test]
    fn test_date_aware_holding_periods() {
        let acq = NaiveDate::from_ymd_opt(2022, 1, 1).unwrap();
        // 800 days holding (> 2 years, < 3 years)
        let pre_budget_disp = NaiveDate::from_ymd_opt(2024, 3, 11).unwrap();
        let post_budget_disp = NaiveDate::from_ymd_opt(2024, 8, 1).unwrap();

        // REIT pre-budget: requires >1095 days -> 800 days is Short-Term
        let reit_pre = determine_holding_period_category(acq, pre_budget_disp, IndianAssetTaxClassification::ReitInvIt);
        assert_eq!(reit_pre, HoldingPeriodCategory::ShortTerm);

        // REIT post-budget: requires >365 days -> 800 days is Long-Term
        let reit_post = determine_holding_period_category(acq, post_budget_disp, IndianAssetTaxClassification::ReitInvIt);
        assert_eq!(reit_post, HoldingPeriodCategory::LongTerm);

        // Gold pre-budget: requires >1095 days -> 800 days is Short-Term
        let gold_pre = determine_holding_period_category(acq, pre_budget_disp, IndianAssetTaxClassification::GoldSgb);
        assert_eq!(gold_pre, HoldingPeriodCategory::ShortTerm);

        // Gold post-budget: requires >730 days -> 800 days is Long-Term
        let gold_post = determine_holding_period_category(acq, post_budget_disp, IndianAssetTaxClassification::GoldSgb);
        assert_eq!(gold_post, HoldingPeriodCategory::LongTerm);
    }
}
