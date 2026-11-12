import { describe, expect, it } from "vitest";
import { ActivityType, ImportFormat } from "@/lib/constants";
import { createDraftActivities, draftToActivityImport } from "./draft-utils";

const headers = ["Date", "Type", "Symbol", "Quantity", "Price", "Amount"];
const mapping = {
  fieldMappings: {
    [ImportFormat.DATE]: "Date",
    [ImportFormat.ACTIVITY_TYPE]: "Type",
    [ImportFormat.SYMBOL]: "Symbol",
    [ImportFormat.QUANTITY]: "Quantity",
    [ImportFormat.UNIT_PRICE]: "Price",
    [ImportFormat.AMOUNT]: "Amount",
  },
  activityMappings: {
    [ActivityType.BUY]: ["BUY"],
    [ActivityType.SELL]: ["SELL"],
  } as Record<string, string[]>,
  symbolMappings: {},
  accountMappings: {},
};
const parseConfig = {
  dateFormat: "YYYY-MM-DD",
  decimalSeparator: ".",
  thousandsSeparator: ",",
  defaultCurrency: "INR",
};
const draft = (row: string[]) =>
  createDraftActivities([row], headers, mapping, parseConfig, "acc-1")[0];

describe("buys at a price of 0 (bonus shares)", () => {
  it("imports a buy at an explicit price of 0 with a total of 0", () => {
    const d = draft(["2022-03-21", "BUY", "EXAMPLE", "132", "0", ""]);
    expect(d.errors.unitPrice).toBeUndefined();
    expect(d.status).not.toBe("error");
    expect(d.amount).toBe("0");
    expect(draftToActivityImport(d).amount).toBe("0");
  });

  it("still needs a price when it is missing", () => {
    const d = draft(["2022-03-21", "BUY", "EXAMPLE", "132", "", ""]);
    expect(d.errors.unitPrice).toBeDefined();
  });

  it("still needs a price above 0 for a sale", () => {
    const d = draft(["2022-03-21", "SELL", "EXAMPLE", "3", "0", ""]);
    expect(d.errors.unitPrice).toBeDefined();
  });
});
