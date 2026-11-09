/**
 * Income tax for a resident individual under 60, by financial year and
 * regime, with capital gains at special rates (Sections 111A, 112A, 112).
 *
 * Covers slabs, the standard deduction, the Section 87A rebate (against slab
 * tax only), the basic exemption adjustment for special-rate gains,
 * surcharge (capped at 15% on special-rate gains, 25% under the new regime)
 * with marginal relief, and 4% cess. It is an estimate: age-based
 * exemptions, AMT and other special-rate income are not covered.
 */

import type { IndianTaxSection } from "@/lib/types";

export type TaxRegime = "old" | "new";

export interface SpecialRateGain {
  section: IndianTaxSection;
  /** e.g. 0.125 */
  rate: number;
  amount: number;
}

interface Slab {
  /** Upper end of the slab; the last one is Infinity. */
  upTo: number;
  rate: number;
}

const OLD_SLABS: Slab[] = [
  { upTo: 250_000, rate: 0 },
  { upTo: 500_000, rate: 0.05 },
  { upTo: 1_000_000, rate: 0.2 },
  { upTo: Infinity, rate: 0.3 },
];

/** New regime, FY 2023-24. */
const NEW_SLABS_2023: Slab[] = [
  { upTo: 300_000, rate: 0 },
  { upTo: 600_000, rate: 0.05 },
  { upTo: 900_000, rate: 0.1 },
  { upTo: 1_200_000, rate: 0.15 },
  { upTo: 1_500_000, rate: 0.2 },
  { upTo: Infinity, rate: 0.3 },
];

/** New regime, FY 2024-25 (Finance (No. 2) Act 2024). */
const NEW_SLABS_2024: Slab[] = [
  { upTo: 300_000, rate: 0 },
  { upTo: 700_000, rate: 0.05 },
  { upTo: 1_000_000, rate: 0.1 },
  { upTo: 1_200_000, rate: 0.15 },
  { upTo: 1_500_000, rate: 0.2 },
  { upTo: Infinity, rate: 0.3 },
];

/** New regime, FY 2025-26 onwards (Finance Act 2025). */
const NEW_SLABS_2025: Slab[] = [
  { upTo: 400_000, rate: 0 },
  { upTo: 800_000, rate: 0.05 },
  { upTo: 1_200_000, rate: 0.1 },
  { upTo: 1_600_000, rate: 0.15 },
  { upTo: 2_000_000, rate: 0.2 },
  { upTo: 2_400_000, rate: 0.25 },
  { upTo: Infinity, rate: 0.3 },
];

export function slabsFor(fyStartYear: number, regime: TaxRegime): Slab[] {
  if (regime === "old") return OLD_SLABS;
  if (fyStartYear >= 2025) return NEW_SLABS_2025;
  if (fyStartYear === 2024) return NEW_SLABS_2024;
  return NEW_SLABS_2023;
}

export function standardDeduction(fyStartYear: number, regime: TaxRegime): number {
  return regime === "new" && fyStartYear >= 2024 ? 75_000 : 50_000;
}

/** Section 87A: total income limit and the most the rebate can be. */
export function rebateRule(
  fyStartYear: number,
  regime: TaxRegime,
): { incomeLimit: number; maxRebate: number } {
  if (regime === "old") return { incomeLimit: 500_000, maxRebate: 12_500 };
  if (fyStartYear >= 2025) return { incomeLimit: 1_200_000, maxRebate: 60_000 };
  return { incomeLimit: 700_000, maxRebate: 25_000 };
}

const SURCHARGE_THRESHOLDS = [5_000_000, 10_000_000, 20_000_000, 50_000_000];

export function surchargeRate(totalIncome: number, regime: TaxRegime): number {
  if (totalIncome > 50_000_000) return regime === "new" ? 0.25 : 0.37;
  if (totalIncome > 20_000_000) return 0.25;
  if (totalIncome > 10_000_000) return 0.15;
  if (totalIncome > 5_000_000) return 0.1;
  return 0;
}

/** Surcharge on tax at special rates is capped at 15%. */
const SPECIAL_RATE_SURCHARGE_CAP = 0.15;

function slabTax(income: number, slabs: Slab[]): number {
  let tax = 0;
  let lower = 0;
  for (const slab of slabs) {
    if (income <= lower) break;
    tax += (Math.min(income, slab.upTo) - lower) * slab.rate;
    lower = slab.upTo;
  }
  return tax;
}

export interface RegimeTax {
  /** Slab income plus special-rate gains. */
  totalIncome: number;
  slabIncome: number;
  specialRateIncome: number;
  /** Unused basic exemption set against special-rate gains. */
  basicExemptionAdjustment: number;
  slabTax: number;
  specialRateTax: number;
  rebate87a: number;
  surcharge: number;
  marginalRelief: number;
  cess: number;
  total: number;
}

type TaxBeforeCess = Omit<RegimeTax, "marginalRelief" | "cess" | "total"> & {
  beforeCess: number;
};

function taxBeforeCess(
  fyStartYear: number,
  regime: TaxRegime,
  slabIncome: number,
  gains: SpecialRateGain[],
): TaxBeforeCess {
  const slabs = slabsFor(fyStartYear, regime);
  const specialRateIncome = gains.reduce((sum, g) => sum + Math.max(0, g.amount), 0);
  const totalIncome = slabIncome + specialRateIncome;

  // Unused basic exemption reduces special-rate gains, highest rate first.
  let shortfall = Math.max(0, slabs[0].upTo - slabIncome);
  let basicExemptionAdjustment = 0;
  let specialRateTax = 0;
  for (const g of [...gains].sort((a, b) => b.rate - a.rate)) {
    const used = Math.min(shortfall, Math.max(0, g.amount));
    shortfall -= used;
    basicExemptionAdjustment += used;
    specialRateTax += (Math.max(0, g.amount) - used) * g.rate;
  }

  const tax = slabTax(slabIncome, slabs);
  const { incomeLimit, maxRebate } = rebateRule(fyStartYear, regime);
  let rebate87a = 0;
  if (totalIncome <= incomeLimit) {
    rebate87a = Math.min(tax, maxRebate);
  } else if (regime === "new") {
    // Marginal relief: tax may not exceed the income above the limit.
    const excess = totalIncome - incomeLimit;
    rebate87a = Math.min(tax, Math.max(0, tax + specialRateTax - excess));
  }

  const rate = surchargeRate(totalIncome, regime);
  const surcharge =
    (tax - rebate87a) * rate + specialRateTax * Math.min(rate, SPECIAL_RATE_SURCHARGE_CAP);

  return {
    totalIncome,
    slabIncome,
    specialRateIncome,
    basicExemptionAdjustment,
    slabTax: tax,
    specialRateTax,
    rebate87a,
    surcharge,
    beforeCess: tax - rebate87a + specialRateTax + surcharge,
  };
}

/** Takes `amount` off slab income first, then off the lowest-rate gains. */
function reduceIncome(
  slabIncome: number,
  gains: SpecialRateGain[],
  amount: number,
): { slabIncome: number; gains: SpecialRateGain[] } {
  const fromSlab = Math.min(slabIncome, amount);
  let left = amount - fromSlab;
  const reduced = [...gains]
    .sort((a, b) => a.rate - b.rate)
    .map((g) => {
      const cut = Math.min(left, Math.max(0, g.amount));
      left -= cut;
      return { ...g, amount: g.amount - cut };
    });
  return { slabIncome: slabIncome - fromSlab, gains: reduced };
}

/**
 * Tax for one regime. `slabIncome` is income taxed at slab rates after the
 * standard deduction and any deductions (which can't reduce special-rate
 * gains).
 */
export function computeRegimeTax(
  fyStartYear: number,
  regime: TaxRegime,
  slabIncome: number,
  gains: SpecialRateGain[],
): RegimeTax {
  const income = Math.max(0, slabIncome);
  const base = taxBeforeCess(fyStartYear, regime, income, gains);

  // Marginal relief on surcharge: crossing a threshold may not cost more
  // than the income above it.
  let marginalRelief = 0;
  const threshold = [...SURCHARGE_THRESHOLDS].reverse().find((t) => base.totalIncome > t);
  if (threshold !== undefined) {
    const excess = base.totalIncome - threshold;
    const atThreshold = reduceIncome(income, gains, excess);
    const taxAtThreshold = taxBeforeCess(
      fyStartYear,
      regime,
      atThreshold.slabIncome,
      atThreshold.gains,
    ).beforeCess;
    marginalRelief = Math.min(
      base.surcharge,
      Math.max(0, base.beforeCess - (taxAtThreshold + excess)),
    );
  }

  const afterRelief = base.beforeCess - marginalRelief;
  const cess = afterRelief * 0.04;
  return {
    totalIncome: base.totalIncome,
    slabIncome: base.slabIncome,
    specialRateIncome: base.specialRateIncome,
    basicExemptionAdjustment: base.basicExemptionAdjustment,
    slabTax: base.slabTax,
    specialRateTax: base.specialRateTax,
    rebate87a: base.rebate87a,
    surcharge: base.surcharge,
    marginalRelief,
    cess,
    total: afterRelief + cess,
  };
}

/** Parses the summary's `specialRateGains` (decimal strings from Rust). */
export function parseSpecialRateGains(
  gains: { section: IndianTaxSection; rate: string; amount: string }[] | undefined,
): SpecialRateGain[] {
  return (gains ?? [])
    .map((g) => ({
      section: g.section,
      rate: parseFloat(g.rate) || 0,
      amount: parseFloat(g.amount) || 0,
    }))
    .filter((g) => g.amount > 0);
}
