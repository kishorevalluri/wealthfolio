import { describe, expect, it } from "vitest";

import { HoldingsFormat } from "./holdings-mapping-step";
import {
  buildHoldingsRowResolutionMap,
  parseHoldingsSnapshots,
  parseHoldingsSnapshotsForValidation,
} from "../utils/holdings-import-utils";
import type { DraftActivity } from "../context";

function createDraft(overrides: Partial<DraftActivity>): DraftActivity {
  return {
    rowIndex: 0,
    rawRow: [],
    activityDate: "2000-01-01",
    activityType: "BUY",
    currency: "USD",
    accountId: "acc-1",
    quantity: "1",
    unitPrice: "1",
    status: "valid",
    errors: {},
    warnings: {},
    isEdited: false,
    ...overrides,
  };
}

describe("holdings review helpers", () => {
  it("maps created asset ids back onto row resolutions", () => {
    const resolutions = buildHoldingsRowResolutionMap(
      [
        createDraft({
          rowIndex: 2,
          symbol: "VOO",
          exchangeMic: "ARCX",
          importAssetKey: "asset-key-1",
        }),
      ],
      {
        "asset-key-1": "asset-123",
      },
    );

    expect(resolutions[2]).toEqual({
      symbol: "VOO",
      exchangeMic: "ARCX",
      assetId: "asset-123",
    });
  });

  it("maps created asset ids from candidate keys back onto row resolutions", () => {
    const resolutions = buildHoldingsRowResolutionMap(
      [
        createDraft({
          rowIndex: 4,
          symbol: "SHOP",
          exchangeMic: "XTSE",
          assetCandidateKey: "candidate-key-1",
        }),
      ],
      {
        "candidate-key-1": "asset-shop-tsx",
      },
    );

    expect(resolutions[4]).toEqual({
      symbol: "SHOP",
      exchangeMic: "XTSE",
      assetId: "asset-shop-tsx",
    });
  });

  it("carries provider refs through row resolutions and snapshots", () => {
    const resolutions = buildHoldingsRowResolutionMap([
      createDraft({
        rowIndex: 0,
        symbol: "SHOP",
        exchangeMic: "XTSE",
        quoteCcy: "CAD",
        instrumentType: "EQUITY",
        quoteMode: "MARKET",
        providerId: "YAHOO",
        providerSymbol: "SHOP.TO",
      }),
    ]);

    expect(resolutions[0]).toEqual({
      symbol: "SHOP",
      exchangeMic: "XTSE",
      quoteCcy: "CAD",
      instrumentType: "EQUITY",
      quoteMode: "MARKET",
      providerId: "YAHOO",
      providerSymbol: "SHOP.TO",
    });

    const snapshots = parseHoldingsSnapshots(
      ["date", "symbol", "quantity", "currency"],
      [["2026-01-02", "SHOP.TO", "10", "CAD"]],
      {
        [HoldingsFormat.DATE]: "date",
        [HoldingsFormat.SYMBOL]: "symbol",
        [HoldingsFormat.QUANTITY]: "quantity",
        [HoldingsFormat.CURRENCY]: "currency",
      },
      {
        dateFormat: "YYYY-MM-DD",
        decimalSeparator: ".",
        thousandsSeparator: ",",
        defaultCurrency: "USD",
      },
      undefined,
      undefined,
      resolutions,
    );

    expect(snapshots[0].positions[0]).toMatchObject({
      symbol: "SHOP",
      exchangeMic: "XTSE",
      quoteCcy: "CAD",
      instrumentType: "EQUITY",
      quoteMode: "MARKET",
      providerId: "YAHOO",
      providerSymbol: "SHOP.TO",
    });
  });

  it("keeps row-level resolutions distinct for duplicate raw symbols", () => {
    const snapshots = parseHoldingsSnapshots(
      ["date", "symbol", "quantity", "currency"],
      [
        ["2026-01-02", "SHOP", "10", "USD"],
        ["2026-01-02", "SHOP", "5", "CAD"],
      ],
      {
        [HoldingsFormat.DATE]: "date",
        [HoldingsFormat.SYMBOL]: "symbol",
        [HoldingsFormat.QUANTITY]: "quantity",
        [HoldingsFormat.CURRENCY]: "currency",
      },
      {
        dateFormat: "YYYY-MM-DD",
        decimalSeparator: ".",
        thousandsSeparator: ",",
        defaultCurrency: "USD",
      },
      undefined,
      undefined,
      {
        0: { symbol: "SHOP", exchangeMic: "XNYS", assetId: "shop-nyse" },
        1: { symbol: "SHOP", exchangeMic: "XTSE", assetId: "shop-tsx" },
      },
    );

    expect(snapshots).toHaveLength(1);
    expect(snapshots[0].positions).toEqual([
      {
        symbol: "SHOP",
        quantity: "10",
        avgCost: undefined,
        currency: "USD",
        exchangeMic: "XNYS",
        assetId: "shop-nyse",
      },
      {
        symbol: "SHOP",
        quantity: "5",
        avgCost: undefined,
        currency: "CAD",
        exchangeMic: "XTSE",
        assetId: "shop-tsx",
      },
    ]);
  });

  it("adds up rows for the same security instead of keeping only the last", () => {
    const snapshots = parseHoldingsSnapshots(
      ["date", "symbol", "quantity", "avgCost", "currency"],
      [
        // One fund in two folios, as a depository statement lists it.
        ["2026-10-31", "INF000000CC1", "100.1", "120", "INR"],
        ["2026-10-31", "INF000000CC1", "200.2", "125", "INR"],
        ["2026-10-31", "INE000000AA1", "30", "", "INR"],
        ["2026-10-31", "INE000000AA1", "30", "", "INR"],
      ],
      {
        [HoldingsFormat.DATE]: "date",
        [HoldingsFormat.SYMBOL]: "symbol",
        [HoldingsFormat.QUANTITY]: "quantity",
        [HoldingsFormat.AVG_COST]: "avgCost",
        [HoldingsFormat.CURRENCY]: "currency",
      },
      {
        dateFormat: "YYYY-MM-DD",
        decimalSeparator: ".",
        thousandsSeparator: ",",
        defaultCurrency: "INR",
      },
    );

    expect(snapshots[0].positions).toEqual([
      // (100.1 × 120 + 200.2 × 125) / 300.3
      { symbol: "INF000000CC1", quantity: "300.3", avgCost: "123.333333", currency: "INR" },
      { symbol: "INE000000AA1", quantity: "60", avgCost: undefined, currency: "INR" },
    ]);
  });

  it("preserves malformed rows in their date group for backend validation", () => {
    const snapshots = parseHoldingsSnapshotsForValidation(
      ["date", "symbol", "quantity", "avgCost", "currency"],
      [
        ["2026-01-02", "AAPL", "10", "125", "USD"],
        ["2026-01-02", "MSFT", "not-a-number", "invalid-cost", "USD"],
        ["not-a-date", "GOOG", "5", "100", "USD"],
      ],
      {
        [HoldingsFormat.DATE]: "date",
        [HoldingsFormat.SYMBOL]: "symbol",
        [HoldingsFormat.QUANTITY]: "quantity",
        [HoldingsFormat.AVG_COST]: "avgCost",
        [HoldingsFormat.CURRENCY]: "currency",
      },
      {
        dateFormat: "YYYY-MM-DD",
        decimalSeparator: ".",
        thousandsSeparator: ",",
        defaultCurrency: "USD",
      },
    );

    expect(snapshots).toHaveLength(2);
    expect(snapshots.find((snapshot) => snapshot.date === "2026-01-02")?.positions).toEqual([
      expect.objectContaining({ symbol: "AAPL", quantity: "10", avgCost: "125" }),
      expect.objectContaining({
        symbol: "MSFT",
        quantity: "not-a-number",
        avgCost: "invalid-cost",
      }),
    ]);
    expect(snapshots.find((snapshot) => snapshot.date === "not-a-date")?.positions).toEqual([
      expect.objectContaining({ symbol: "GOOG", quantity: "5" }),
    ]);
  });

  it("represents malformed cash amounts as validation positions", () => {
    const snapshots = parseHoldingsSnapshotsForValidation(
      ["date", "symbol", "quantity", "currency"],
      [["2026-01-02", "$CASH", "invalid", "USD"]],
      {
        [HoldingsFormat.DATE]: "date",
        [HoldingsFormat.SYMBOL]: "symbol",
        [HoldingsFormat.QUANTITY]: "quantity",
        [HoldingsFormat.CURRENCY]: "currency",
      },
      {
        dateFormat: "YYYY-MM-DD",
        decimalSeparator: ".",
        thousandsSeparator: ",",
        defaultCurrency: "USD",
      },
    );

    expect(snapshots[0].cashBalances).toEqual({});
    expect(snapshots[0].positions).toEqual([
      expect.objectContaining({ symbol: "$CASH", quantity: "invalid" }),
    ]);
  });
});
