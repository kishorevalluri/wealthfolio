import { describe, expect, it } from "vitest";
import type { DraftActivity } from "../context/import-context";
import {
  findBalanceMismatches,
  parseStatementDate,
  statementBalancesFromRows,
} from "./statement-balances";

function draft(overrides: Partial<DraftActivity>): DraftActivity {
  return {
    rowIndex: 0,
    rawRow: [],
    activityDate: "2026-08-10T00:00:00.000Z",
    activityType: "BUY",
    currency: "INR",
    accountId: "acc",
    status: "valid",
    errors: {},
    warnings: {},
    isEdited: false,
    ...overrides,
  };
}

describe("statementBalancesFromRows", () => {
  it("reads the CDSL holdings rows only", () => {
    const rows = [
      [
        "31-08-2026",
        "HOLDING BALANCE",
        "GOLDBEES",
        "INF000000AA1",
        "1,700",
        "",
        "",
        "",
        "",
        "",
        "",
      ],
      ["12-08-2026", "BUY", "GOLDBEES", "INF000000AA1", "65", "", "", "", "", "", ""],
    ];
    expect(statementBalancesFromRows(rows)).toEqual([
      { isin: "INF000000AA1", name: "GOLDBEES", quantity: 1700, date: "2026-08-31" },
    ]);
  });
});

describe("parseStatementDate", () => {
  it("reads day-first dates", () => {
    expect(parseStatementDate("05-08-2026")).toBe("2026-08-05");
    expect(parseStatementDate("5/8/26")).toBe("2026-08-05");
    expect(parseStatementDate("31-Aug-2026")).toBe("2026-08-31");
    expect(parseStatementDate("2026-08-31")).toBe("");
  });
});

describe("findBalanceMismatches", () => {
  const balances = [
    { isin: "INE000000AA1", name: "EMS", quantity: 60, date: "2026-08-31" },
    { isin: "INE000000BB2", name: "XYZ", quantity: 10, date: "2026-08-31" },
  ];

  it("counts holdings, later trades and the rows this import adds", () => {
    const result = findBalanceMismatches({
      balances,
      // 70 held today, 20 bought after the statement date.
      holdings: [{ assetId: "ems", isin: "INE000000AA1", quantity: 70 }],
      laterActivities: [{ assetId: "ems", activityType: "BUY", quantity: "20" }],
      drafts: [
        // Matched to a broker trade: not imported, not counted.
        draft({ isin: "INE000000AA1", quantity: "50", status: "duplicate" }),
        draft({ isin: "INE000000AA1", quantity: "10", activityType: "TRANSFER_IN" }),
        draft({ isin: "INE000000BB2", quantity: "4", status: "skipped" }),
      ],
    });
    expect(result).toEqual([{ isin: "INE000000BB2", name: "XYZ", statement: 10, account: 0 }]);
  });

  it("counts a duplicate the user chose to import anyway", () => {
    const result = findBalanceMismatches({
      balances: [balances[1]],
      holdings: [],
      laterActivities: [],
      drafts: [
        draft({ isin: "INE000000BB2", quantity: "10", status: "duplicate", forceImport: true }),
      ],
    });
    expect(result).toEqual([]);
  });
});
