/**
 * Indian tax details stored in asset metadata, mirroring the Rust readers in
 * `crates/core/src/lots/tax_service.rs`:
 * - `taxClassification` — snake_case `IndianAssetTaxClassification`
 * - `fmv20180131` — fair market value per unit on 31 Jan 2018, for
 *   Section 55(2)(ac) grandfathering
 *
 * Rust also accepts the older keys `assetTaxClassification`, `fmv_2018_01_31`
 * and `fmv2018`. They are read here too, and removed on save so only one
 * value can apply.
 */

import type { IndianAssetTaxClassification } from "@/lib/types";

export const INDIAN_TAX_CLASSIFICATIONS: readonly IndianAssetTaxClassification[] = [
  "equity_listed",
  "equity_mf",
  "reit_invit",
  "debt_mf",
  "non_equity_etf",
  "non_equity_fund",
  "unlisted_debenture_post_2024",
  "gold_sgb",
  "other_capital_asset",
];

const CLASSIFICATION_KEYS = ["taxClassification", "assetTaxClassification"] as const;
const FMV_KEYS = ["fmv20180131", "fmv_2018_01_31", "fmv2018"] as const;

/** Spellings Rust accepts beyond the canonical snake_case value. */
const CLASSIFICATION_ALIASES: Record<string, IndianAssetTaxClassification> = {
  equitylisted: "equity_listed",
  equitymf: "equity_mf",
  reitinvit: "reit_invit",
  debtmf: "debt_mf",
  debt_mf_post_2023: "debt_mf",
  debtmfpost2023: "debt_mf",
  nonequityetf: "non_equity_etf",
  nonequityfund: "non_equity_fund",
  unlisteddebenturepost2024: "unlisted_debenture_post_2024",
  othercapitalasset: "other_capital_asset",
  goldsgb: "gold_sgb",
};

type Metadata = Record<string, unknown>;

export interface IndianTaxFormValues {
  /** Empty string means "automatic": no override is stored. */
  indianTaxClassification: string;
  fmv2018: number | null;
}

function asObject(value: unknown): Metadata | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Metadata;
}

function parseClassification(value: unknown): IndianAssetTaxClassification | null {
  if (typeof value !== "string") return null;
  const lower = value.trim().toLowerCase();
  if ((INDIAN_TAX_CLASSIFICATIONS as readonly string[]).includes(lower)) {
    return lower as IndianAssetTaxClassification;
  }
  return CLASSIFICATION_ALIASES[lower] ?? null;
}

function toNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

/** Reads the stored values, using the first key Rust would read. */
export function extractIndianTax(metadata: unknown): IndianTaxFormValues {
  const meta = asObject(metadata);
  const classKey = CLASSIFICATION_KEYS.find((k) => meta?.[k] !== undefined);
  const fmvKey = FMV_KEYS.find((k) => meta?.[k] !== undefined);
  return {
    indianTaxClassification: (classKey && parseClassification(meta?.[classKey])) || "",
    fmv2018: fmvKey ? toNumber(meta?.[fmvKey]) : null,
  };
}

/**
 * Returns metadata with the values applied, without mutating the input.
 * The FMV is stored as a string so the decimal value is kept exactly.
 */
export function applyIndianTax(metadata: Metadata, values: IndianTaxFormValues): Metadata {
  const next: Metadata = { ...metadata };
  for (const key of [...CLASSIFICATION_KEYS, ...FMV_KEYS]) delete next[key];

  const classification = parseClassification(values.indianTaxClassification);
  if (classification) next.taxClassification = classification;

  if (values.fmv2018 != null && Number.isFinite(values.fmv2018) && values.fmv2018 > 0) {
    next.fmv20180131 = String(values.fmv2018);
  }
  return next;
}
