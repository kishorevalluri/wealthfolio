import { describe, expect, it } from "vitest";
import type { DraftActivity } from "../context/import-context";
import {
  balancesByAccount,
  findBalanceMismatches,
  parseStatementDate,
  rowsForStatementAccount,
  statementAccounts,
  statementBalancesFromRows,
  statementHoldingRows,
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
      {
        isin: "INF000000AA1",
        name: "GOLDBEES",
        account: "",
        quantity: 1700,
        date: "2026-08-31",
      },
    ]);
  });

  it("adds up one fund held in two folios of the same account", () => {
    const row = (units: string, account: string) => [
      "31-10-2026",
      "HOLDING BALANCE",
      "MID CAP FUND",
      "INF000000CC1",
      units,
      "",
      "",
      account,
      "",
      "",
      "",
    ];
    const result = statementBalancesFromRows([
      row("100.1", "MF FOLIOS"),
      row("200.2", "MF FOLIOS"),
      row("100", "MF FOLIOS (JOINT: B)"),
    ]);
    expect(result.map((b) => [b.account, b.quantity])).toEqual([
      ["MF FOLIOS", 300.3],
      ["MF FOLIOS (JOINT: B)", 100],
    ]);
  });
});

describe("balancesByAccount", () => {
  const b = (account: string, isin: string, quantity: number) => ({
    isin,
    name: "",
    account,
    quantity,
    date: "2026-10-31",
  });
  const balances = [
    b("1208160000000001", "INE000000AA1", 30),
    b("1208180000000002", "INE000000AA1", 30),
    b("1208180000000002", "INE000000BB2", 5),
    b("MF FOLIOS", "INF000000CC1", 10),
  ];
  const row = (account: string) => ["", "", "", "", "", "", "", account];

  it("checks each statement account against the account its rows go to", () => {
    const groups = balancesByAccount({
      balances,
      drafts: [
        { rawRow: row("1208160000000001"), accountId: "zerodha" },
        { rawRow: row("MF FOLIOS"), accountId: "mf" },
        { rawRow: row("MF FOLIOS"), accountId: "mf" },
      ],
      accountColumn: 7,
      defaultAccountId: "zerodha",
    });
    // The second demat account has no rows in this import: not checked.
    expect([...groups.entries()]).toEqual([
      ["zerodha", [b("1208160000000001", "INE000000AA1", 30)]],
      ["mf", [b("MF FOLIOS", "INF000000CC1", 10)]],
    ]);
  });

  it("follows rows moved to another account in review", () => {
    const groups = balancesByAccount({
      balances: [balances[3]],
      drafts: [
        { rawRow: row("MF FOLIOS"), accountId: "joint" },
        { rawRow: row("MF FOLIOS"), accountId: "joint" },
        { rawRow: row("MF FOLIOS"), accountId: "mf" },
      ],
      accountColumn: 7,
      defaultAccountId: "zerodha",
    });
    expect([...groups.keys()]).toEqual(["joint"]);
  });

  it("adds up two statement accounts imported into one account", () => {
    const groups = balancesByAccount({
      balances,
      drafts: [
        { rawRow: row("1208160000000001"), accountId: "all" },
        { rawRow: row("1208180000000002"), accountId: "all" },
      ],
      accountColumn: 7,
      defaultAccountId: "all",
    });
    expect(groups.get("all")?.map((x) => [x.isin, x.quantity])).toEqual([
      ["INE000000AA1", 60],
      ["INE000000BB2", 5],
    ]);
  });

  it("checks everything against the selected account without an account column", () => {
    const groups = balancesByAccount({
      balances: [balances[0]],
      drafts: [],
      accountColumn: -1,
      defaultAccountId: "picked",
    });
    expect([...groups.keys()]).toEqual(["picked"]);
  });
});

describe("holdings rows of a statement", () => {
  const row = (isin: string, units: string, account: string) => [
    "31-10-2026",
    "HOLDING BALANCE",
    "X",
    isin,
    units,
    "",
    "",
    account,
    "",
    "",
    "",
  ];
  const rows = [
    ["30-10-2026", "SELL", "X", "INE000000AA1", "130", "", "", "1208160000000001", "", "", ""],
    row("INE000000AA1", "0", "1208160000000001"),
    row("INE000000BB2", "155.000", "1208160000000001"),
    row("INE000000DD4", "90.000", "1208180000000002"),
    row("INF000000CC1", "10", "MF FOLIOS"),
  ];

  it("keeps positions held on the statement date", () => {
    expect(statementHoldingRows(rows).map((r) => r[3])).toEqual([
      "INE000000BB2",
      "INE000000DD4",
      "INF000000CC1",
    ]);
  });

  it("lists statement accounts and filters by one", () => {
    const holdings = statementHoldingRows(rows);
    expect(statementAccounts(holdings)).toEqual([
      "1208160000000001",
      "1208180000000002",
      "MF FOLIOS",
    ]);
    expect(rowsForStatementAccount(holdings, "MF FOLIOS").map((r) => r[3])).toEqual([
      "INF000000CC1",
    ]);
    expect(rowsForStatementAccount(holdings, "")).toHaveLength(3);
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
    { isin: "INE000000AA1", name: "EMS", account: "", quantity: 60, date: "2026-08-31" },
    { isin: "INE000000BB2", name: "XYZ", account: "", quantity: 10, date: "2026-08-31" },
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
