import { describe, expect, it } from "vitest";
import { ACTIVITY_SUBTYPES, ActivityType, ImportFormat } from "@/lib/constants";
import { createDraftActivities, draftToActivityImport } from "./draft-utils";

// Columns as the CDSL e-CAS parser writes them (synthetic data only).
const headers = [
  "Date",
  "Transaction Description",
  "Security Description",
  "ISIN",
  "Quantity",
  "Rate",
  "Amount",
  "BO ID",
];

const mapping = {
  fieldMappings: {
    [ImportFormat.DATE]: "Date",
    [ImportFormat.ACTIVITY_TYPE]: "Transaction Description",
    [ImportFormat.SYMBOL]: "Security Description",
    [ImportFormat.ISIN]: "ISIN",
    [ImportFormat.QUANTITY]: "Quantity",
    [ImportFormat.UNIT_PRICE]: "Rate",
    [ImportFormat.AMOUNT]: "Amount",
    [ImportFormat.ACCOUNT]: "BO ID",
  },
  activityMappings: {
    [ActivityType.BUY]: ["BUY"],
    [ActivityType.SELL]: ["SELL"],
  } as Record<string, string[]>,
  symbolMappings: {},
  accountMappings: { "1200000000000001": "acc-1", "1200000000000002": "acc-2" },
};

const parseConfig = {
  dateFormat: "DD-MM-YYYY",
  decimalSeparator: ".",
  thousandsSeparator: ",",
  defaultCurrency: "INR",
};

const row = (date: string, label: string, name: string, isin: string, qty: string, bo = "1") => [
  date,
  label,
  name,
  isin,
  qty,
  "",
  "",
  `120000000000000${bo}`,
];

function draftsFor(rows: string[][], activityMappings = mapping.activityMappings) {
  return createDraftActivities(
    rows,
    headers,
    { ...mapping, activityMappings },
    parseConfig,
    "acc-1",
    new Set(["acc-1", "acc-2"]),
  );
}

describe("CDSL corporate actions with an ISIN change", () => {
  it("turns a split's old-ISIN debit and new-ISIN credit into one conversion", () => {
    const drafts = draftsFor([
      row("10-03-2026", "BUY", "EXAMPLE STEEL LTD", "INE000A01011", "10"),
      row("22-08-2026", "CORPORATE ACTION DEBIT", "EXAMPLE STEEL LTD", "INE000A01011", "10"),
      row("22-08-2026", "CORPORATE ACTION CREDIT", "EXAMPLE STEEL LTD", "INE000A01029", "50"),
    ]);
    const [, leaving, arriving] = drafts;
    expect(leaving.activityType).toBe(ActivityType.ADJUSTMENT);
    expect(leaving.subtype).toBe(ACTIVITY_SUBTYPES.CONVERSION_OUT);
    expect(arriving.activityType).toBe(ActivityType.ADJUSTMENT);
    expect(arriving.subtype).toBe(ACTIVITY_SUBTYPES.CONVERSION_IN);
    expect(leaving.sourceGroupId).toBeTruthy();
    expect(arriving.sourceGroupId).toBe(leaving.sourceGroupId);
    expect([leaving.quantity, arriving.quantity]).toEqual(["10", "50"]);
    expect(leaving.status).not.toBe("error");
    expect(arriving.status).not.toBe("error");
    // The group reaches the backend, which writes the pair.
    expect(draftToActivityImport(arriving).sourceGroupId).toBe(leaving.sourceGroupId);
    // The same statement imported again gives the same group (duplicate check).
    const again = draftsFor([
      row("22-08-2026", "CORPORATE ACTION DEBIT", "EXAMPLE STEEL LTD", "INE000A01011", "10"),
      row("22-08-2026", "CORPORATE ACTION CREDIT", "EXAMPLE STEEL LTD", "INE000A01029", "50"),
    ]);
    expect(again[0].sourceGroupId).toBe(leaving.sourceGroupId);
  });

  it("pairs a merger into another company when it is the only one that day", () => {
    const drafts = draftsFor([
      row("01-07-2026", "CORPORATE ACTION DEBIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
      row("01-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE HOLDINGS LTD", "INE000C01013", "42"),
    ]);
    expect(drafts.map((d) => d.subtype)).toEqual([
      ACTIVITY_SUBTYPES.CONVERSION_OUT,
      ACTIVITY_SUBTYPES.CONVERSION_IN,
    ]);
  });

  it("matches by issuer when several companies change ISIN the same day", () => {
    const drafts = draftsFor([
      row("05-09-2026", "CORPORATE ACTION DEBIT", "ALPHA LTD", "INE000D01019", "5"),
      row("05-09-2026", "CORPORATE ACTION DEBIT", "BETA LTD", "INE000E01016", "8"),
      row("05-09-2026", "CORPORATE ACTION CREDIT", "BETA LTD", "INE000E01024", "80"),
      row("05-09-2026", "CORPORATE ACTION CREDIT", "ALPHA LTD", "INE000D01027", "25"),
    ]);
    expect(drafts[0].sourceGroupId).toBe(drafts[3].sourceGroupId);
    expect(drafts[1].sourceGroupId).toBe(drafts[2].sourceGroupId);
    expect(drafts[0].sourceGroupId).not.toBe(drafts[1].sourceGroupId);
  });

  it("leaves bonuses, other days, other accounts and mapped rows alone", () => {
    const bonus = draftsFor([
      row("01-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
    ]);
    expect(bonus[0].activityType).toBeFalsy();
    expect(bonus[0].sourceGroupId).toBeUndefined();

    const apart = draftsFor([
      row("01-07-2026", "CORPORATE ACTION DEBIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
      row("02-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE BANK LTD", "INE000B01023", "100"),
      row("01-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE BANK LTD", "INE000B01023", "100", "2"),
    ]);
    expect(apart.every((d) => d.sourceGroupId === undefined)).toBe(true);

    const mapped = draftsFor(
      [
        row("01-07-2026", "CORPORATE ACTION DEBIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
        row("01-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE BANK LTD", "INE000B01023", "100"),
      ],
      { ...mapping.activityMappings, [ActivityType.TRANSFER_IN]: ["CORPORATE ACTION CREDIT"] },
    );
    expect(mapped[1].activityType).toBe(ActivityType.TRANSFER_IN);
    expect(mapped.every((d) => d.sourceGroupId === undefined)).toBe(true);
  });

  it("does not guess for a demerger that credits two companies", () => {
    const drafts = draftsFor([
      row("15-10-2026", "CORPORATE ACTION DEBIT", "PARENT LTD", "INE000F01010", "20"),
      row("15-10-2026", "CORPORATE ACTION CREDIT", "PARENT LTD", "INE000F01028", "20"),
      row("15-10-2026", "CORPORATE ACTION CREDIT", "SPUNOFF LTD", "INE000G01018", "20"),
    ]);
    // The parent's old and new ISIN pair; the resulting company's credit is
    // left for the user, since the cost split needs the company's ratio.
    expect(drafts[0].sourceGroupId).toBe(drafts[1].sourceGroupId);
    expect(drafts[2].sourceGroupId).toBeUndefined();
  });

  it("does not pair a debit and credit of the same ISIN", () => {
    // A same-ISIN debit and credit on one day (e.g. a re-credit after a
    // reversal) is not an ISIN change.
    const drafts = draftsFor([
      row("01-07-2026", "CORPORATE ACTION DEBIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
      row("01-07-2026", "CORPORATE ACTION CREDIT", "EXAMPLE BANK LTD", "INE000B01015", "100"),
    ]);
    expect(drafts.every((d) => d.sourceGroupId === undefined)).toBe(true);
  });
});
