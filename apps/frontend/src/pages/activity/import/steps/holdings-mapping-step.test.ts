import { describe, expect, it } from "vitest";
import { autoDetectHoldingsMapping, HoldingsFormat } from "./holdings-mapping-step";

describe("autoDetectHoldingsMapping", () => {
  it("maps a CDSL statement's ISIN and cost, not its name and market price", () => {
    const mapping = autoDetectHoldingsMapping([
      "Date",
      "Transaction Description",
      "Security Description",
      "ISIN",
      "Quantity",
      "Rate",
      "Amount",
      "BO ID",
      "Fee",
      "Avg Cost",
      "Particulars",
    ]);
    expect(mapping[HoldingsFormat.DATE]).toBe("Date");
    expect(mapping[HoldingsFormat.SYMBOL]).toBe("ISIN");
    expect(mapping[HoldingsFormat.QUANTITY]).toBe("Quantity");
    expect(mapping[HoldingsFormat.AVG_COST]).toBe("Avg Cost");
  });

  it("still uses a price column when there is no cost column", () => {
    const mapping = autoDetectHoldingsMapping(["Date", "Symbol", "Quantity", "Price"]);
    expect(mapping[HoldingsFormat.SYMBOL]).toBe("Symbol");
    expect(mapping[HoldingsFormat.AVG_COST]).toBe("Price");
  });
});
