/**
 * Checks the dividends and sales an AIS lists against what the app worked out
 * for the same financial year, so a difference can be traced to one side.
 *
 * Neither side is taken as right. Each row says what each side has, and a
 * hint says what usually causes that kind of difference.
 */
import type { IndianTaxYearSummary } from "@/lib/types";
import type { AisDividend, AisSale, AisStatement } from "./ais-parser";

export type ReconStatus = "match" | "different" | "ais-only" | "app-only";

/** Words that do not tell companies apart. */
const STOP_WORDS = new Set([
  "LIMITED",
  "LTD",
  "THE",
  "PVT",
  "PRIVATE",
  "CO",
  "CORP",
  "CORPORATION",
  "COMPANY",
  "INC",
  "AND",
  "OF",
  "EQ",
  "EQUITY",
  "SHARES",
  "NEW",
  "FV",
  "RS",
  "RE",
]);

export function nameTokens(name: string): string[] {
  return name
    .toUpperCase()
    .replace(/&/g, " AND ")
    .replace(/[^A-Z0-9 ]/g, " ")
    .split(/\s+/)
    .filter((t) => t && !STOP_WORDS.has(t) && !/^\d+(\.\d+)?$/.test(t));
}

const LEGAL_SUFFIXES = new Set(["LIMITED", "LTD", "THE", "PVT", "PRIVATE", "INC"]);

/** First word whole, then the first letter of each later word. */
function initials(name: string): string {
  const words = name
    .toUpperCase()
    .replace(/&/g, " AND ")
    .replace(/[^A-Z0-9 ]/g, " ")
    .split(/\s+/)
    .filter((t) => t && !LEGAL_SUFFIXES.has(t));
  return words.length > 1
    ? words[0] +
        words
          .slice(1)
          .map((t) => t[0])
          .join("")
    : "";
}

/** How alike two company or security names are, from 0 to 1. */
export function nameScore(a: string, b: string): number {
  const ta = nameTokens(a);
  const tb = nameTokens(b);
  if (!ta.length || !tb.length) return 0;
  const joinedA = ta.join("");
  const joinedB = tb.join("");
  if (joinedA === joinedB) return 1;
  const setB = new Set(tb);
  const common = ta.filter((t) => setB.has(t)).length;
  let score = (2 * common) / (ta.length + tb.length);
  // "HDFCAMC" (a symbol) against "HDFC ASSET MANAGEMENT COMPANY": initials.
  if (joinedA === initials(b) || joinedB === initials(a)) score = Math.max(score, 0.9);
  return score;
}

const NAME_THRESHOLD = 0.6;

function num(value: string | undefined | null): number {
  const n = parseFloat(value ?? "");
  return Number.isFinite(n) ? n : 0;
}

/** Amounts agree to the rupee (AIS rounds), or 0.1% on large values. */
export function sameAmount(a: number, b: number): boolean {
  return Math.abs(a - b) <= Math.max(1, Math.abs(b) * 0.001);
}

function sameQuantity(a: number | undefined, b: number): boolean {
  return a === undefined || Math.abs(a - b) < 0.001;
}

function daysBetween(a: string, b: string): number {
  return Math.round((Date.parse(b) - Date.parse(a)) / 86_400_000);
}

// ── Sales ───────────────────────────────────────────────────────────────

export interface AppSaleLot {
  acquisitionDate: string;
  quantity: number;
  cost: number;
  deemedCost: number;
  gain: number;
}

/** The app's sales of one security on one day. */
export interface AppSale {
  assetId: string;
  name: string;
  isin?: string;
  date: string;
  quantity: number;
  consideration: number;
  cost: number;
  /** Cost after 31 Jan 2018 grandfathering or indexation. */
  deemedCost: number;
  gain: number;
  intraday: boolean;
  lots: AppSaleLot[];
}

export interface SaleRow {
  status: ReconStatus;
  name: string;
  isin?: string;
  date: string;
  /** AIS date when it is not the app's (settlement date, a late report). */
  aisDate?: string;
  ais?: {
    quantity?: number;
    consideration: number;
    cost?: number;
    entries: AisSale[];
    /** Reported more than once with the same values (by two reporters). */
    duplicated: boolean;
  };
  app?: AppSale;
  hints: string[];
}

export interface ScripTotals {
  name: string;
  isin?: string;
  status: ReconStatus;
  aisQuantity: number;
  aisConsideration: number;
  appQuantity: number;
  appConsideration: number;
  appGain: number;
  rows: SaleRow[];
}

/** Groups the app's capital-gain rows and intraday trades by security and day. */
export function appSales(summary: IndianTaxYearSummary): AppSale[] {
  const groups = new Map<string, AppSale>();
  for (const c of summary.calculations) {
    const date = c.disposalDate.slice(0, 10);
    const key = `${c.assetId}|${date}`;
    const quantity = num(c.quantity);
    const price = num(c.salePricePerUnit);
    const consideration = price > 0 ? quantity * price : num(c.proceeds);
    let sale = groups.get(key);
    if (!sale) {
      sale = {
        assetId: c.assetId,
        name: c.assetName || c.assetSymbol || c.assetId,
        isin: c.isin || undefined,
        date,
        quantity: 0,
        consideration: 0,
        cost: 0,
        deemedCost: 0,
        gain: 0,
        intraday: false,
        lots: [],
      };
      groups.set(key, sale);
    }
    sale.quantity += quantity;
    sale.consideration += consideration;
    sale.cost += num(c.actualCost);
    sale.deemedCost += num(c.deemedCost);
    sale.gain += num(c.grossGain);
    sale.lots.push({
      acquisitionDate: c.acquisitionDate.slice(0, 10),
      quantity,
      cost: num(c.actualCost),
      deemedCost: num(c.deemedCost),
      gain: num(c.grossGain),
    });
  }
  for (const t of summary.speculativeTrades ?? []) {
    const date = t.date.slice(0, 10);
    const key = `${t.assetId}|${date}`;
    const sale = groups.get(key) ?? {
      assetId: t.assetId,
      name: t.assetName || t.assetId,
      isin: t.isin || undefined,
      date,
      quantity: 0,
      consideration: 0,
      cost: 0,
      deemedCost: 0,
      gain: 0,
      intraday: true,
      lots: [],
    };
    sale.intraday = true;
    sale.quantity += num(t.quantity);
    // Intraday proceeds are net of the sell charges; close enough to compare.
    sale.consideration += num(t.proceeds);
    sale.cost += num(t.cost);
    sale.deemedCost += num(t.cost);
    sale.gain += num(t.profit);
    groups.set(key, sale);
  }
  return [...groups.values()].sort((a, b) => a.date.localeCompare(b.date));
}

interface AisSaleGroup {
  name: string;
  isin?: string;
  date: string;
  quantity?: number;
  consideration: number;
  cost?: number;
  entries: AisSale[];
  duplicated: boolean;
}

/** Groups AIS sales by security and day, folding in a second reporter's copy. */
function groupAisSales(sales: AisSale[]): AisSaleGroup[] {
  const groups = new Map<string, AisSale[]>();
  for (const s of sales) {
    const key = `${s.isin ?? nameTokens(s.name).join(" ")}|${s.date}`;
    groups.set(key, [...(groups.get(key) ?? []), s]);
  }
  return [...groups.values()].map((entries) => {
    const byReporter = new Map<string, AisSale[]>();
    for (const e of entries) {
      const r = e.reportedBy ?? "";
      byReporter.set(r, [...(byReporter.get(r) ?? []), e]);
    }
    const totals = [...byReporter.values()].map((list) => ({
      quantity: list.every((e) => e.quantity !== undefined)
        ? list.reduce((s, e) => s + (e.quantity ?? 0), 0)
        : undefined,
      consideration: list.reduce((s, e) => s + e.consideration, 0),
      cost: list.every((e) => e.cost !== undefined)
        ? list.reduce((s, e) => s + (e.cost ?? 0), 0)
        : undefined,
    }));
    const duplicated =
      totals.length > 1 &&
      totals.every(
        (t) =>
          sameAmount(t.consideration, totals[0].consideration) &&
          (t.quantity ?? 0) === (totals[0].quantity ?? 0),
      );
    const used = duplicated ? [totals[0]] : totals;
    const sum = (f: (t: (typeof totals)[0]) => number | undefined) =>
      used.every((t) => f(t) !== undefined) ? used.reduce((s, t) => s + (f(t) ?? 0), 0) : undefined;
    return {
      name: entries[0].name,
      isin: entries.find((e) => e.isin)?.isin,
      date: entries[0].date,
      quantity: sum((t) => t.quantity),
      consideration: sum((t) => t.consideration) ?? 0,
      cost: sum((t) => t.cost),
      entries,
      duplicated,
    };
  });
}

/** Which app security an AIS security is: same ISIN, else the closest name. */
function scripKeyFor(
  ais: { isin?: string; name: string },
  app: Map<string, { name: string; isin?: string }>,
): string | undefined {
  if (ais.isin) {
    for (const [key, s] of app) if (s.isin === ais.isin) return key;
  }
  let best: string | undefined;
  let bestScore = NAME_THRESHOLD;
  for (const [key, s] of app) {
    if (ais.isin && s.isin && ais.isin !== s.isin) continue;
    const score = nameScore(ais.name, s.name);
    if (score >= bestScore) {
      best = key;
      bestScore = score;
    }
  }
  return best;
}

function saleHints(row: SaleRow): string[] {
  const hints: string[] = [];
  const { ais, app } = row;
  if (ais?.duplicated) {
    hints.push(
      "AIS lists this sale twice (two reporters with the same values). Counted once here.",
    );
  }
  if (row.status === "ais-only") {
    hints.push(
      /-04-0[1-7]$/.test(row.date)
        ? "AIS dates sales by settlement, so a sale traded in the last days of March (the previous year in the app) can show up here. Check the previous year's sales."
        : "Not in the app for this day. Either the sale was not imported, or AIS shows an off-market transfer, a buyback or a delisting as a sale.",
    );
  }
  if (row.status === "app-only") {
    hints.push(
      app?.intraday
        ? "Intraday trade. Brokers often leave these out of AIS."
        : /-03-(2[4-9]|3[01])$/.test(row.date)
          ? "Settled in April, so AIS may list it in next year's statement."
          : "AIS does not list this sale. The broker or depository may not have reported it, or reported it under another date or year.",
    );
  }
  // One day later is the normal settlement gap; only a longer one is worth a note.
  if (ais && app && row.aisDate && Math.abs(daysBetween(row.date, row.aisDate)) > 1) {
    hints.push(
      `AIS dates this ${Math.abs(daysBetween(row.date, row.aisDate))} day(s) ${
        row.aisDate > row.date ? "later" : "earlier"
      }, usually the settlement date.`,
    );
  }
  if (ais && app && row.status === "different") {
    if (ais.quantity !== undefined && !sameQuantity(ais.quantity, app.quantity)) {
      hints.push(
        `Quantities differ: AIS ${ais.quantity}, app ${app.quantity}. Check for a split, bonus or partial import.`,
      );
    } else if (!sameAmount(ais.consideration, app.consideration)) {
      const qty = app.quantity || 1;
      hints.push(
        `Sale value differs: AIS ₹${(ais.consideration / qty).toFixed(2)} a unit, app ₹${(
          app.consideration / qty
        ).toFixed(2)}. AIS sometimes uses the closing price or adds charges.`,
      );
    }
  }
  if (ais?.cost !== undefined && app) {
    if (!sameAmount(ais.cost, app.cost) && !sameAmount(ais.cost, app.deemedCost)) {
      hints.push(
        ais.cost === 0
          ? "AIS gives the cost as ₹0: the reporter did not know it. Use the app's cost."
          : `Cost differs: AIS ₹${Math.round(ais.cost).toLocaleString("en-IN")}, app ₹${Math.round(
              app.cost,
            ).toLocaleString("en-IN")}${
              app.deemedCost !== app.cost
                ? ` (₹${Math.round(app.deemedCost).toLocaleString("en-IN")} after grandfathering)`
                : ""
            }. AIS cost is often from the depository, which may not know transfers in.`,
      );
    }
  }
  return hints;
}

/** Matches AIS sales to the app's sales, security by security. */
export function reconcileSales(aisSales: AisSale[], app: AppSale[]): ScripTotals[] {
  const appScrips = new Map<string, { name: string; isin?: string; sales: AppSale[] }>();
  for (const s of app) {
    const entry = appScrips.get(s.assetId) ?? { name: s.name, isin: s.isin, sales: [] };
    entry.sales.push(s);
    appScrips.set(s.assetId, entry);
  }
  const aisByScrip = new Map<string, AisSaleGroup[]>();
  const unmatchedAis = new Map<string, AisSaleGroup[]>();
  for (const g of groupAisSales(aisSales)) {
    const key = scripKeyFor(g, appScrips);
    if (key) aisByScrip.set(key, [...(aisByScrip.get(key) ?? []), g]);
    else {
      const k = g.isin ?? nameTokens(g.name).join(" ");
      unmatchedAis.set(k, [...(unmatchedAis.get(k) ?? []), g]);
    }
  }

  const result: ScripTotals[] = [];
  const toRow = (g: AisSaleGroup, a: AppSale | undefined): SaleRow => {
    const row: SaleRow = {
      status: "ais-only",
      name: a?.name ?? g.name,
      isin: a?.isin ?? g.isin,
      date: a?.date ?? g.date,
      aisDate: g.date,
      ais: {
        quantity: g.quantity,
        consideration: g.consideration,
        cost: g.cost,
        entries: g.entries,
        duplicated: g.duplicated,
      },
      app: a,
      hints: [],
    };
    if (a) {
      row.status =
        sameQuantity(g.quantity, a.quantity) && sameAmount(g.consideration, a.consideration)
          ? "match"
          : "different";
    }
    row.hints = saleHints(row);
    return row;
  };

  for (const [key, scrip] of appScrips) {
    const rows: SaleRow[] = [];
    const left = [...scrip.sales];
    const aisGroups = aisByScrip.get(key) ?? [];
    // Depositories report the day the shares left the demat account, usually
    // the trade day plus one settlement day (more over holidays), so pair each
    // AIS sale with the app sale it fits best: same quantity and value first,
    // then the shortest gap from the trade day forward.
    const pairs: { g: number; a: number; score: number }[] = [];
    aisGroups.forEach((g, gi) => {
      left.forEach((a, ai) => {
        const gap = daysBetween(a.date, g.date);
        if (gap < -1 || gap > 7) return;
        const sameQty = sameQuantity(g.quantity, a.quantity);
        const sameValue = sameAmount(g.consideration, a.consideration);
        // A different quantity only pairs on the same day: otherwise it is
        // more likely another sale.
        if (!sameQty && gap !== 0) return;
        const fit = sameQty && sameValue ? 0 : sameQty ? 1 : 2;
        pairs.push({ g: gi, a: ai, score: fit * 100 + (gap >= 0 ? gap : 5) });
      });
    });
    pairs.sort((x, y) => x.score - y.score);
    const usedG = new Set<number>();
    const usedA = new Set<number>();
    for (const p of pairs) {
      if (usedG.has(p.g) || usedA.has(p.a)) continue;
      usedG.add(p.g);
      usedA.add(p.a);
      rows.push(toRow(aisGroups[p.g], left[p.a]));
    }
    aisGroups.forEach((g, gi) => {
      if (!usedG.has(gi)) rows.push(toRow(g, undefined));
    });
    const unpaired = left.filter((_, ai) => !usedA.has(ai));
    left.length = 0;
    left.push(...unpaired);
    for (const a of left) {
      const row: SaleRow = {
        status: "app-only",
        name: a.name,
        isin: a.isin,
        date: a.date,
        app: a,
        hints: [],
      };
      row.hints = saleHints(row);
      rows.push(row);
    }
    result.push(totals(scrip.name, scrip.isin, rows));
  }
  for (const groups of unmatchedAis.values()) {
    const rows = groups.map((g) => toRow(g, undefined));
    result.push(totals(groups[0].name, groups[0].isin, rows));
  }
  return result.sort(
    (a, b) => statusRank(a.status) - statusRank(b.status) || a.name.localeCompare(b.name),
  );
}

function statusRank(status: ReconStatus): number {
  return { different: 0, "ais-only": 1, "app-only": 2, match: 3 }[status];
}

function totals(name: string, isin: string | undefined, rows: SaleRow[]): ScripTotals {
  rows.sort((a, b) => a.date.localeCompare(b.date));
  const statuses = new Set(rows.map((r) => r.status));
  // A security with some days matching and some not is "different" overall.
  const status: ReconStatus = statuses.size === 1 ? rows[0].status : "different";
  return {
    name,
    isin,
    status,
    aisQuantity: rows.reduce((s, r) => s + (r.ais?.quantity ?? 0), 0),
    aisConsideration: rows.reduce((s, r) => s + (r.ais?.consideration ?? 0), 0),
    appQuantity: rows.reduce((s, r) => s + (r.app?.quantity ?? 0), 0),
    appConsideration: rows.reduce((s, r) => s + (r.app?.consideration ?? 0), 0),
    appGain: rows.reduce((s, r) => s + (r.app?.gain ?? 0), 0),
    rows,
  };
}

// ── Dividends ───────────────────────────────────────────────────────────

export interface AppDividend {
  assetId: string;
  name: string;
  symbol?: string;
  gross: number;
  tds: number;
  payments: { date: string; gross: number; tds: number }[];
}

export interface DividendRow {
  status: ReconStatus;
  /** The company as the app names it, or as AIS does when only AIS has it. */
  name: string;
  aisCompany?: string;
  /** The AIS names folded into this row (more than one after a rename). */
  aisCompanies?: string[];
  ais?: {
    gross: number;
    tds: number;
    entries: AisDividend[];
    /** Gross from the company's own report (SFT-015) and from its TDS (TDS-194). */
    bySource: { code: string; amount: number; tds: number }[];
  };
  app?: AppDividend;
  hints: string[];
}

export function appDividends(summary: IndianTaxYearSummary): AppDividend[] {
  return (summary.otherSources?.dividendsByAsset ?? []).map((d) => ({
    assetId: d.assetId,
    name: d.name || d.symbol || d.assetId,
    symbol: d.symbol ?? undefined,
    gross: num(d.gross),
    tds: num(d.tds),
    payments: (d.payments ?? []).map((p) => ({
      date: p.date.slice(0, 10),
      gross: num(p.gross),
      tds: num(p.tds),
    })),
  }));
}

/**
 * The gross AIS claims for one company. A company that deducted TDS shows up
 * under both SFT-015 and TDS-194 for the same dividends, so the two are not
 * added: the larger is taken, and a gap between them is pointed out.
 */
function aisCompanyTotals(entries: AisDividend[]) {
  const bySourceMap = new Map<string, { code: string; amount: number; tds: number }>();
  for (const e of entries) {
    const code = e.code.includes("194") ? "TDS-194" : /015|15/.test(e.code) ? "SFT-015" : e.code;
    const s = bySourceMap.get(code) ?? { code, amount: 0, tds: 0 };
    s.amount += e.amount;
    s.tds += e.tds;
    bySourceMap.set(code, s);
  }
  const bySource = [...bySourceMap.values()];
  const gross = Math.max(...bySource.map((s) => s.amount));
  const tds = Math.max(...bySource.map((s) => s.tds));
  return { gross, tds, bySource };
}

function dividendHints(row: DividendRow): string[] {
  const hints: string[] = [];
  const { ais, app } = row;
  if (ais && ais.bySource.length > 1) {
    const [a, b] = ais.bySource;
    if (!sameAmount(a.amount, b.amount)) {
      hints.push(
        `AIS itself disagrees: ${a.code} says ₹${Math.round(a.amount).toLocaleString(
          "en-IN",
        )}, ${b.code} says ₹${Math.round(b.amount).toLocaleString("en-IN")}. The larger is used.`,
      );
    }
  }
  if (row.status === "ais-only") {
    hints.push(
      "Not recorded in the app. Add it, or use Corporate actions on the Activities page to propose dividends from NSE.",
    );
  }
  if (row.status === "app-only") {
    hints.push(
      "AIS does not list this dividend. Companies often skip SFT-015 when no TDS was due, so this is usually AIS missing it.",
    );
  }
  if (ais && app && row.status === "different") {
    if (!sameAmount(ais.gross, app.gross)) {
      const ratio = app.gross ? ais.gross / app.gross : 0;
      if (Math.abs(ratio - 2) < 0.02) {
        hints.push("AIS is exactly double the app: the company likely reported it twice.");
      } else if (ais.gross > app.gross) {
        hints.push(
          "AIS is higher. A dividend may be missing in the app, or AIS includes shares held jointly or in another account.",
        );
      } else {
        hints.push(
          "AIS is lower. A dividend in the app may be dated in another year in AIS, or the company has not reported it.",
        );
      }
    }
    if (!sameAmount(ais.tds, app.tds)) {
      hints.push(
        `TDS differs: AIS ₹${Math.round(ais.tds).toLocaleString("en-IN")}, app ₹${Math.round(
          app.tds,
        ).toLocaleString("en-IN")}. Form 26AS is the record the tax department credits.`,
      );
    }
  }
  return hints;
}

/**
 * Matches AIS dividends to the app's, company by company. `manual` maps an
 * AIS company name to an app asset id where the names are too different to
 * match on their own.
 */
export function reconcileDividends(
  aisDividends: AisDividend[],
  app: AppDividend[],
  manual: Record<string, string> = {},
): DividendRow[] {
  const companies = new Map<string, AisDividend[]>();
  for (const d of aisDividends) {
    const key = nameTokens(d.company).join(" ") || d.company;
    companies.set(key, [...(companies.get(key) ?? []), d]);
  }
  const byId = new Map(app.map((a) => [a.assetId, a]));
  // Each AIS company goes to one app asset. Several can go to the same one:
  // AIS keeps a renamed company's old name (Jubilant Life Sciences, now
  // Jubilant Pharmova) next to the new one.
  const toAsset = new Map<string, AisDividend[][]>();
  const unmatched: AisDividend[][] = [];
  for (const entries of companies.values()) {
    const company = entries[0].company;
    let match = manual[company] ? byId.get(manual[company]) : undefined;
    if (!match) {
      let bestScore = NAME_THRESHOLD;
      for (const a of app) {
        const score = Math.max(
          nameScore(company, a.name),
          a.symbol ? nameScore(company, a.symbol) : 0,
        );
        if (score > bestScore || (score === bestScore && !match)) {
          match = a;
          bestScore = score;
        }
      }
    }
    if (match) toAsset.set(match.assetId, [...(toAsset.get(match.assetId) ?? []), entries]);
    else unmatched.push(entries);
  }
  const rows: DividendRow[] = [];
  const addRow = (groups: AisDividend[][], match: AppDividend | undefined) => {
    const entries = groups.flat();
    const names = groups.map((g) => g[0].company);
    const totals = aisCompanyTotals(entries);
    const row: DividendRow = {
      status: !match
        ? "ais-only"
        : sameAmount(totals.gross, match.gross) && sameAmount(totals.tds, match.tds)
          ? "match"
          : "different",
      name: match?.name ?? names[0],
      aisCompany: names.join(" / "),
      aisCompanies: names,
      ais: { ...totals, entries },
      app: match,
      hints: [],
    };
    row.hints = dividendHints(row);
    if (names.length > 1) {
      row.hints.unshift(
        `AIS lists this under ${names.length} names (${names.join(", ")}), usually after a rename. Each source is counted once across them.`,
      );
    }
    rows.push(row);
  };
  for (const [assetId, groups] of toAsset) addRow(groups, byId.get(assetId));
  for (const entries of unmatched) addRow([entries], undefined);
  const appLeft = new Map(app.filter((a) => !toAsset.has(a.assetId)).map((a) => [a.assetId, a]));
  for (const a of appLeft.values()) {
    if (a.gross === 0 && a.tds === 0) continue;
    const row: DividendRow = { status: "app-only", name: a.name, app: a, hints: [] };
    row.hints = dividendHints(row);
    rows.push(row);
  }
  return rows.sort(
    (a, b) => statusRank(a.status) - statusRank(b.status) || a.name.localeCompare(b.name),
  );
}

export interface AisReconciliation {
  dividends: DividendRow[];
  scrips: ScripTotals[];
  totals: {
    aisDividends: number;
    appDividends: number;
    aisDividendTds: number;
    appDividendTds: number;
    aisSaleValue: number;
    appSaleValue: number;
  };
}

/** Keeps the AIS entries dated inside the financial year (undated ones stay). */
function inYear<T extends { date?: string }>(items: T[], fyStartYear: number): T[] {
  const start = `${fyStartYear}-04-01`;
  const end = `${fyStartYear + 1}-03-31`;
  return items.filter((i) => !i.date || (i.date >= start && i.date <= end));
}

export function reconcileAis(
  statement: AisStatement,
  summary: IndianTaxYearSummary,
  manual: Record<string, string> = {},
): AisReconciliation {
  const dividends = reconcileDividends(
    inYear(statement.dividends, summary.fyStartYear),
    appDividends(summary),
    manual,
  );
  const scrips = reconcileSales(inYear(statement.sales, summary.fyStartYear), appSales(summary));
  return {
    dividends,
    scrips,
    totals: {
      aisDividends: dividends.reduce((s, d) => s + (d.ais?.gross ?? 0), 0),
      appDividends: dividends.reduce((s, d) => s + (d.app?.gross ?? 0), 0),
      aisDividendTds: dividends.reduce((s, d) => s + (d.ais?.tds ?? 0), 0),
      appDividendTds: dividends.reduce((s, d) => s + (d.app?.tds ?? 0), 0),
      aisSaleValue: scrips.reduce((s, r) => s + r.aisConsideration, 0),
      appSaleValue: scrips.reduce((s, r) => s + r.appConsideration, 0),
    },
  };
}
