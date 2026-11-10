/**
 * Closing balances from a depository statement (CDSL CAS), checked against
 * what the account will hold once the import is done.
 */

import type { DraftActivity } from "../context/import-context";

/** Label the CDSL parser gives its holdings rows. */
export const HOLDING_BALANCE_LABEL = "HOLDING BALANCE";

export interface StatementBalance {
  isin: string;
  name: string;
  quantity: number;
  /** YYYY-MM-DD */
  date: string;
}

export interface BalanceMismatch {
  isin: string;
  name: string;
  /** Quantity on the statement. */
  statement: number;
  /** Quantity the account will hold on the statement date after this import. */
  account: number;
}

/** Account-side inputs, kept to the fields the check reads. */
export interface HeldPosition {
  assetId?: string;
  isin?: string | null;
  quantity: number;
}

export interface LaterActivity {
  assetId?: string;
  activityType: string;
  quantity: string | number | null;
}

// MF units have 3 decimals.
const TOLERANCE = 0.0005;

function toNumber(value: string | number | null | undefined): number {
  if (typeof value === "number") return value;
  const n = parseFloat((value ?? "").replace(/,/g, ""));
  return Number.isFinite(n) ? n : 0;
}

/** Sign of a row's effect on quantity, or 0 when it has none. */
function quantitySign(activityType: string): number {
  switch (activityType) {
    case "BUY":
    case "TRANSFER_IN":
      return 1;
    case "SELL":
    case "TRANSFER_OUT":
      return -1;
    default:
      return 0;
  }
}

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/**
 * Indian statements put the day first: 31-08-2026, 31/08/26 or 31-Aug-2026.
 * Read as a calendar date, so the time zone can't move it.
 */
export function parseStatementDate(text: string): string {
  const m = /^(\d{1,2})[-/ ]([A-Za-z]{3}|\d{1,2})[-/ ](\d{2}|\d{4})$/.exec(text.trim());
  if (!m) return "";
  const day = Number(m[1]);
  const month = /^\d+$/.test(m[2]) ? Number(m[2]) : MONTHS.indexOf(m[2].toLowerCase()) + 1;
  const year = m[3].length === 2 ? 2000 + Number(m[3]) : Number(m[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return "";
  return `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/**
 * Reads the parser's HOLDING BALANCE rows: Date, label, security, ISIN,
 * quantity (the CDSL statement layout).
 */
export function statementBalancesFromRows(rows: string[][]): StatementBalance[] {
  return rows
    .filter((row) => row[1] === HOLDING_BALANCE_LABEL && row[3]?.trim())
    .map((row) => ({
      isin: row[3].trim().toUpperCase(),
      name: row[2]?.trim() ?? "",
      quantity: toNumber(row[4]),
      date: parseStatementDate(row[0] ?? ""),
    }));
}

function willImport(draft: DraftActivity): boolean {
  if (draft.status === "duplicate") return !!draft.forceImport;
  return draft.status === "valid" || draft.status === "warning";
}

/**
 * Compares each statement balance with the account: today's holdings, less
 * activity after the statement date, plus the rows this import adds.
 */
export function findBalanceMismatches({
  balances,
  holdings,
  laterActivities,
  drafts,
}: {
  balances: StatementBalance[];
  holdings: HeldPosition[];
  laterActivities: LaterActivity[];
  drafts: DraftActivity[];
}): BalanceMismatch[] {
  const isinByAsset = new Map<string, string>();
  const held = new Map<string, number>();
  for (const h of holdings) {
    const isin = h.isin?.trim().toUpperCase();
    if (!isin) continue;
    if (h.assetId) isinByAsset.set(h.assetId, isin);
    held.set(isin, (held.get(isin) ?? 0) + h.quantity);
  }
  for (const d of drafts) {
    if (d.assetId && d.isin) isinByAsset.set(d.assetId, d.isin.trim().toUpperCase());
  }
  const add = (isin: string | undefined, delta: number) => {
    if (!isin || delta === 0) return;
    held.set(isin, (held.get(isin) ?? 0) + delta);
  };

  for (const a of laterActivities) {
    const isin = a.assetId ? isinByAsset.get(a.assetId) : undefined;
    add(isin, -quantitySign(a.activityType) * Math.abs(toNumber(a.quantity)));
  }
  for (const d of drafts) {
    if (!willImport(d)) continue;
    const isin =
      d.isin?.trim().toUpperCase() || (d.assetId ? isinByAsset.get(d.assetId) : undefined);
    add(isin, quantitySign(d.activityType) * Math.abs(toNumber(d.quantity)));
  }

  return balances
    .map((b) => ({
      isin: b.isin,
      name: b.name,
      statement: b.quantity,
      account: held.get(b.isin) ?? 0,
    }))
    .filter((m) => Math.abs(m.statement - m.account) > TOLERANCE);
}
