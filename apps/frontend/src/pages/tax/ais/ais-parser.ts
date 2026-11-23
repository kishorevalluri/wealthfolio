/**
 * Reads the dividend and sale-of-securities entries out of an Annual
 * Information Statement (AIS) downloaded from the income-tax portal.
 *
 * The portal offers the AIS as JSON (for its offline utility) and each
 * information category as CSV. Neither layout is documented, so rather than
 * expect fixed paths this reads every table it finds (arrays of objects in
 * JSON, header rows in CSV) and recognises columns by their names. Fields a
 * table lacks, such as the information code or the reporting company, are
 * taken from the objects around it.
 *
 * The AIS values are only read here, to be checked against the app. Nothing
 * is imported from them.
 */

/** A dividend the AIS lists: from the company (SFT-015) or its TDS (TDS-194). */
export interface AisDividend {
  /** Information code as the AIS gives it, such as "TDS-194" or "SFT-015". */
  code: string;
  /** The company that paid it (the reporting source or deductor). */
  company: string;
  /** YYYY-MM-DD, when the AIS gives one. */
  date?: string;
  amount: number;
  tds: number;
}

/** A sale of shares or fund units the AIS lists (SFT-017/018 or similar). */
export interface AisSale {
  code: string;
  /** YYYY-MM-DD */
  date: string;
  name: string;
  isin?: string;
  quantity?: number;
  unitPrice?: number;
  /** Sale value: quantity × price, before charges. */
  consideration: number;
  /** Cost of acquisition, when the reporter gives one. */
  cost?: number;
  /** Who reported it: a depository, broker or registrar. */
  reportedBy?: string;
}

export interface AisStatement {
  /** Financial year start, when the file says (2025 = FY 2025-26). */
  fyStartYear?: number;
  dividends: AisDividend[];
  sales: AisSale[];
  /** Tables that looked relevant but could not be read, for the person to see. */
  skipped: number;
}

type Row = Record<string, string>;

/** Lower case, letters and digits only: "Date of Sale/Transfer" → "dateofsaletransfer". */
function norm(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

const FIELDS = {
  code: ["informationcode", "infocode", "infcode", "sftcode", "code", "sectioncode", "section"],
  description: [
    "informationdescription",
    "infodesc",
    "informationcategory",
    "infocategory",
    "category",
    "description",
    "natureoftransaction",
    "transactiontype",
    "transactiondescription",
    "naturetype",
  ],
  source: [
    "informationsource",
    "infosource",
    "sourcename",
    "nameofdeductor",
    "deductorname",
    "nameofthedeductor",
    "reportingentity",
    "reportingentityname",
    "nameofreportingentity",
    "filername",
    "nameofthefiler",
    "reportedby",
    "companyname",
    "nameofcompany",
    "nameofthecompany",
  ],
  date: [
    "dateofsaletransfer",
    "dateofsale",
    "dateoftransfer",
    "saledate",
    "transactiondate",
    "dateoftransaction",
    "dateofpaymentcredit",
    "dateofpayment",
    "dateofcredit",
    "paymentdate",
    "creditdate",
    "date",
  ],
  name: [
    "securityname",
    "nameofsecurity",
    "nameofthesecurity",
    "securitydescription",
    "scripname",
    "nameofscrip",
    "schemename",
    "nameofscheme",
    "securityclassname",
  ],
  isin: ["isin", "isincode", "securitycode", "scripcode", "securityid"],
  quantity: [
    "quantity",
    "quantitysold",
    "qty",
    "units",
    "noofunits",
    "noofshares",
    "numberofunits",
  ],
  unitPrice: [
    "salepriceperunit",
    "salespriceperunit",
    "salesprice",
    "saleprice",
    "priceperunit",
    "rate",
    "nav",
  ],
  consideration: [
    "salesconsideration",
    "saleconsideration",
    "salevalue",
    "salesvalue",
    "totalsalesvalue",
    "totalsaleconsideration",
    "fullvalueofconsideration",
    "considerationamount",
    "consideration",
  ],
  cost: ["costofacquisition", "purchasevalue", "purchaseconsideration", "acquisitioncost"],
  amount: [
    "amountpaidcredited",
    "amountpaidorcredited",
    "amountpaid",
    "amountcredited",
    "dividendamount",
    "grossamount",
    "amountofdividend",
    "transactionamount",
    "amount",
    "value",
  ],
  tds: [
    "taxdeducted",
    "tdsdeducted",
    "tdsamount",
    "tds",
    "taxdeductedcollected",
    "amountoftaxdeducted",
    "taxdeposited",
  ],
} as const;

type Field = keyof typeof FIELDS;

function pick(row: Row, field: Field): string | undefined {
  for (const key of FIELDS[field]) {
    const value = row[key];
    if (value !== undefined && value.trim() !== "") return value.trim();
  }
  return undefined;
}

/** "1,23,456.70", "₹ 1,234", "(123)" and "-" all read as numbers or nothing. */
export function parseAmount(value: string | undefined): number | undefined {
  if (value === undefined) return undefined;
  let text = value.replace(/[₹,\s]|INR|Rs\.?/gi, "");
  let sign = 1;
  if (/^\(.*\)$/.test(text)) {
    sign = -1;
    text = text.slice(1, -1);
  }
  if (!/^-?\d*\.?\d+$/.test(text)) return undefined;
  return sign * Number(text);
}

const MONTHS: Record<string, number> = {
  jan: 1,
  feb: 2,
  mar: 3,
  apr: 4,
  may: 5,
  jun: 6,
  jul: 7,
  aug: 8,
  sep: 9,
  oct: 10,
  nov: 11,
  dec: 12,
};

function iso(y: number, m: number, d: number): string | undefined {
  if (y < 100) y += 2000;
  const date = new Date(Date.UTC(y, m - 1, d));
  if (date.getUTCFullYear() !== y || date.getUTCMonth() !== m - 1 || date.getUTCDate() !== d) {
    return undefined;
  }
  return `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
}

/** AIS dates are Indian: 15/07/2025, 15-07-2025, 15-Jul-2025, or ISO. */
export function parseAisDate(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const text = value.trim();
  let m = /^(\d{4})-(\d{2})-(\d{2})/.exec(text);
  if (m) return iso(+m[1], +m[2], +m[3]);
  m = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2,4})/.exec(text);
  if (m) return iso(+m[3], +m[2], +m[1]);
  m = /^(\d{1,2})[\s/-]([A-Za-z]{3})[A-Za-z]*[\s/,-]+(\d{2,4})/.exec(text);
  if (m && MONTHS[m[2].toLowerCase()]) return iso(+m[3], MONTHS[m[2].toLowerCase()], +m[1]);
  return undefined;
}

function context(row: Row): string {
  return [pick(row, "code"), pick(row, "description"), row.__context]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function isDividendContext(text: string): boolean {
  return /dividend|tds-?194\b|sft-?015|\b194\b(?![a-z])/.test(text);
}

function isSaleContext(text: string): boolean {
  return /sale of (securities|units|shares)|sft-?0?1[78]|sale|redemption|transfer of/.test(text);
}

function codeOf(row: Row, fallback: string): string {
  const code = pick(row, "code");
  if (code) return code.toUpperCase();
  const m = /(sft|tds)-?\s?(\d{3}[a-z]*)/i.exec(row.__context ?? "");
  return m ? `${m[1].toUpperCase()}-${m[2].toUpperCase()}` : fallback;
}

/** Adds one table row to the statement if it is a dividend or a sale. */
function classify(row: Row, out: AisStatement) {
  const text = context(row);
  const date = parseAisDate(pick(row, "date"));
  const name = pick(row, "name");
  const consideration = parseAmount(pick(row, "consideration"));
  const quantity = parseAmount(pick(row, "quantity"));
  const unitPrice = parseAmount(pick(row, "unitPrice"));
  const hasSaleShape =
    date !== undefined &&
    (consideration !== undefined || (quantity !== undefined && unitPrice !== undefined)) &&
    (name !== undefined || pick(row, "isin") !== undefined);

  if (hasSaleShape && (isSaleContext(text) || consideration !== undefined)) {
    if (isDividendContext(text) && consideration === undefined) return;
    const isinRaw = pick(row, "isin");
    const isin = isinRaw && /^IN[A-Z0-9]{10}$/i.test(isinRaw) ? isinRaw.toUpperCase() : undefined;
    out.sales.push({
      code: codeOf(row, "SALE"),
      date,
      name: name ?? isinRaw ?? "",
      isin,
      quantity: quantity !== undefined ? Math.abs(quantity) : undefined,
      unitPrice,
      consideration: Math.abs(consideration ?? (quantity ?? 0) * (unitPrice ?? 0)),
      cost: parseAmount(pick(row, "cost")),
      reportedBy: pick(row, "source"),
    });
    return;
  }

  if (isDividendContext(text)) {
    const amount = parseAmount(pick(row, "amount"));
    const company = pick(row, "source") ?? name;
    if (amount === undefined || !company) return;
    out.dividends.push({
      code: codeOf(row, "DIVIDEND"),
      company,
      date,
      amount: Math.abs(amount),
      tds: Math.abs(parseAmount(pick(row, "tds")) ?? 0),
    });
  }
}

function emptyStatement(): AisStatement {
  return { dividends: [], sales: [], skipped: 0 };
}

function findFy(text: string): number | undefined {
  const m = /(?:financial year|fy|f\.y\.)\s*:?\s*(20\d{2})\s*-\s*(\d{2,4})/i.exec(text);
  if (m) return +m[1];
  const ay = /(?:assessment year|ay|a\.y\.)\s*:?\s*(20\d{2})\s*-\s*(\d{2,4})/i.exec(text);
  return ay ? +ay[1] - 1 : undefined;
}

// ── JSON ────────────────────────────────────────────────────────────────

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function scalars(obj: Record<string, unknown>): Row {
  const row: Row = {};
  for (const [key, value] of Object.entries(obj)) {
    if (value === null || value === undefined) continue;
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      row[norm(key)] = String(value);
    }
  }
  return row;
}

function walk(node: unknown, inherited: Row, keyPath: string, out: AisStatement) {
  if (Array.isArray(node)) {
    for (const item of node) walk(item, inherited, keyPath, out);
    return;
  }
  if (!isPlainObject(node)) return;
  const own = scalars(node);
  // A parent's fields describe its children (the code, the reporting company),
  // but a child's own fields win.
  const row: Row = { ...inherited, ...own };
  row.__context = [inherited.__context, keyPath, pick(own, "code"), pick(own, "description")]
    .filter(Boolean)
    .join(" ");
  const children = Object.entries(node).filter(([, v]) => Array.isArray(v) || isPlainObject(v));
  if (children.length === 0 || Object.keys(own).length >= 3) classify(row, out);
  for (const [key, child] of children) walk(child, row, key, out);
}

export function parseAisJson(text: string): AisStatement {
  const out = emptyStatement();
  const data: unknown = JSON.parse(text);
  out.fyStartYear = findFy(text.slice(0, 5000));
  walk(data, {}, "", out);
  return dedupe(out);
}

// ── CSV ─────────────────────────────────────────────────────────────────

/** Splits CSV text into rows, honouring quotes. */
export function splitCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"') quoted = true;
    else if (c === ",") {
      row.push(cell);
      cell = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
    } else cell += c;
  }
  if (cell !== "" || row.length) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

const KNOWN_HEADERS = new Set<string>(Object.values(FIELDS).flat());

/**
 * A CSV may hold several tables, each under its own header row, with title
 * lines (such as "SFT-018 Sale of securities") in between that set the context.
 */
export function parseAisCsv(text: string, fileName = ""): AisStatement {
  const out = emptyStatement();
  out.fyStartYear = findFy(text.slice(0, 5000)) ?? findFy(fileName);
  let headers: string[] | null = null;
  // The title lines since the last table: they describe the next one.
  let titles: string[] = [fileName];
  let inTable = false;
  for (const cells of splitCsv(text.replace(/^﻿/, ""))) {
    const filled = cells.map((c) => c.trim()).filter(Boolean);
    if (filled.length === 0) continue;
    const known = cells.filter((c) => KNOWN_HEADERS.has(norm(c))).length;
    if (known >= 3) {
      headers = cells.map(norm);
      inTable = true;
      continue;
    }
    if (!headers || filled.length <= 2) {
      // A title or a "key: value" line between tables.
      if (inTable) titles = [fileName];
      inTable = false;
      titles.push(filled.join(" "));
      continue;
    }
    const row: Row = { __context: titles.join(" ") };
    headers.forEach((h, i) => {
      if (h && cells[i] !== undefined) row[h] = cells[i];
    });
    classify(row, out);
  }
  return dedupe(out);
}

/** The same JSON can list a transaction at two levels; keep one copy. */
function dedupe(statement: AisStatement): AisStatement {
  const seen = new Set<string>();
  statement.sales = statement.sales.filter((s) => {
    const key = [s.code, s.date, s.isin ?? s.name, s.quantity, s.consideration, s.reportedBy].join(
      "|",
    );
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  seen.clear();
  statement.dividends = statement.dividends.filter((d) => {
    const key = [d.code, d.company, d.date, d.amount, d.tds].join("|");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  return statement;
}

/** Reads an AIS file by its contents: JSON if it parses as JSON, else CSV. */
export function parseAisFile(text: string, fileName = ""): AisStatement {
  const trimmed = text.trimStart();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    return parseAisJson(trimmed);
  }
  return parseAisCsv(text, fileName);
}
