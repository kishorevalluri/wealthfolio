import { describe, expect, it } from "vitest";
import { ActivityType, ImportFormat } from "@/lib/constants";
import { initializeColumnMapping } from "../hooks/use-import-mapping";
import { findMappedActivityType } from "./activity-type-mapping";
import type { ImportTemplateData } from "@/lib/types";

describe("Indian Broker & CAS Statement Import Mappings", () => {
  // Template definitions matching crates/storage-sqlite/migrations/2026-09-19-000001_indian_broker_templates/up.sql
  const zerodhaTemplate: ImportTemplateData = {
    id: "system_zerodha",
    name: "Zerodha Kite",
    scope: "SYSTEM",
    kind: "CSV_ACTIVITY",
    fieldMappings: {
      [ImportFormat.DATE]: "trade_date",
      [ImportFormat.ACTIVITY_TYPE]: "trade_type",
      [ImportFormat.SYMBOL]: "symbol",
      [ImportFormat.ISIN]: "isin",
      [ImportFormat.QUANTITY]: "quantity",
      [ImportFormat.UNIT_PRICE]: "price",
      [ImportFormat.COMMENT]: "trade_id",
      [ImportFormat.INSTRUMENT_TYPE]: "segment",
    },
    activityMappings: {
      [ActivityType.BUY]: ["buy"],
      [ActivityType.SELL]: ["sell"],
    },
    symbolMappings: {},
    accountMappings: {},
    symbolMappingMeta: {},
  };

  const growwTemplate: ImportTemplateData = {
    id: "system_groww",
    name: "Groww",
    scope: "SYSTEM",
    kind: "CSV_ACTIVITY",
    fieldMappings: {
      [ImportFormat.DATE]: "Execution Date",
      [ImportFormat.ACTIVITY_TYPE]: "Type",
      [ImportFormat.SYMBOL]: "Symbol",
      [ImportFormat.ISIN]: "ISIN",
      [ImportFormat.QUANTITY]: "Quantity",
      [ImportFormat.UNIT_PRICE]: "Price",
      [ImportFormat.AMOUNT]: "Order Value",
      [ImportFormat.COMMENT]: "Order Status",
    },
    activityMappings: {
      [ActivityType.BUY]: ["BUY"],
      [ActivityType.SELL]: ["SELL"],
    },
    symbolMappings: {},
    accountMappings: {},
    symbolMappingMeta: {},
  };

  const camsCasTemplate: ImportTemplateData = {
    id: "system_cams_cas",
    name: "CAMS / KFintech CAS",
    scope: "SYSTEM",
    kind: "CSV_ACTIVITY",
    fieldMappings: {
      [ImportFormat.DATE]: "Date",
      [ImportFormat.ACTIVITY_TYPE]: "Transaction Description",
      [ImportFormat.SYMBOL]: "Scheme Name",
      [ImportFormat.ISIN]: "ISIN",
      [ImportFormat.QUANTITY]: "Units",
      [ImportFormat.UNIT_PRICE]: "NAV",
      [ImportFormat.AMOUNT]: "Amount",
      [ImportFormat.ACCOUNT]: "Folio No",
    },
    activityMappings: {
      [ActivityType.BUY]: [
        "PURCHASE",
        "SIP",
        "ADDITIONAL PURCHASE",
        "SWITCH IN",
        "SWITCH_IN",
        "SYSTEMATIC INVESTMENT",
        "REINVESTMENT",
        "REINVEST SHARES",
        "DRIP",
      ],
      [ActivityType.SELL]: [
        "REDEMPTION",
        "SWITCH OUT",
        "SWITCH_OUT",
        "SWP",
        "SYSTEMATIC WITHDRAWAL",
      ],
      [ActivityType.DIVIDEND]: [
        "DIVIDEND PAYOUT",
        "DIVIDEND REINVESTMENT",
        "DIVIDEND",
        "DIV",
      ],
      [ActivityType.TAX]: [
        "STAMP DUTY",
        "STAMP_DUTY",
        "STT",
        "SECURITIES TRANSACTION TAX",
        "TDS",
        "WITHHOLDING TAX",
      ],
      [ActivityType.FEE]: [
        "BROKERAGE",
        "TRANSACTION CHARGES",
        "CHARGES",
      ],
    },
    symbolMappings: {},
    accountMappings: {},
    symbolMappingMeta: {},
  };

  describe("Zerodha Kite Tradebook", () => {
    const zerodhaHeaders = [
      "symbol",
      "isin",
      "trade_date",
      "exchange",
      "segment",
      "series",
      "trade_type",
      "quantity",
      "price",
      "trade_id",
      "order_id",
      "order_execution_time",
    ];

    it("auto-detects Zerodha Tradebook columns correctly via aliases", () => {
      const mapping = initializeColumnMapping(zerodhaHeaders);

      expect(mapping[ImportFormat.DATE]).toBe("trade_date");
      expect(mapping[ImportFormat.ACTIVITY_TYPE]).toBe("trade_type");
      expect(mapping[ImportFormat.SYMBOL]).toBe("symbol");
      expect(mapping[ImportFormat.ISIN]).toBe("isin");
      expect(mapping[ImportFormat.QUANTITY]).toBe("quantity");
      expect(mapping[ImportFormat.UNIT_PRICE]).toBe("price");
      expect(mapping[ImportFormat.INSTRUMENT_TYPE]).toBe("segment");
      expect(mapping[ImportFormat.COMMENT]).toBe("trade_id");
    });

    it("maps buy/sell trade types using Zerodha template mappings", () => {
      expect(findMappedActivityType("buy", zerodhaTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("BUY", zerodhaTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("sell", zerodhaTemplate.activityMappings)).toBe(ActivityType.SELL);
      expect(findMappedActivityType("SELL", zerodhaTemplate.activityMappings)).toBe(ActivityType.SELL);
    });
  });

  describe("Groww Tradebook", () => {
    const growwHeaders = [
      "Execution Date",
      "Type",
      "Symbol",
      "ISIN",
      "Quantity",
      "Price",
      "Order Value",
      "Order Status",
    ];

    it("auto-detects Groww Tradebook columns correctly via aliases", () => {
      const mapping = initializeColumnMapping(growwHeaders);

      expect(mapping[ImportFormat.DATE]).toBe("Execution Date");
      expect(mapping[ImportFormat.ACTIVITY_TYPE]).toBe("Type");
      expect(mapping[ImportFormat.SYMBOL]).toBe("Symbol");
      expect(mapping[ImportFormat.ISIN]).toBe("ISIN");
      expect(mapping[ImportFormat.QUANTITY]).toBe("Quantity");
      expect(mapping[ImportFormat.UNIT_PRICE]).toBe("Price");
      expect(mapping[ImportFormat.AMOUNT]).toBe("Order Value");
      expect(mapping[ImportFormat.COMMENT]).toBe("Order Status");
    });

    it("maps trade types using Groww template mappings", () => {
      expect(findMappedActivityType("BUY", growwTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("SELL", growwTemplate.activityMappings)).toBe(ActivityType.SELL);
    });
  });

  describe("CAMS / KFintech Consolidated Account Statement (CAS)", () => {
    const camsHeaders = [
      "Date",
      "Transaction Description",
      "Amount",
      "Units",
      "NAV",
      "Folio No",
      "Scheme Name",
      "ISIN",
    ];

    it("auto-detects CAMS/KFintech CAS columns correctly via aliases", () => {
      const mapping = initializeColumnMapping(camsHeaders);

      expect(mapping[ImportFormat.DATE]).toBe("Date");
      expect(mapping[ImportFormat.ACTIVITY_TYPE]).toBe("Transaction Description");
      expect(mapping[ImportFormat.AMOUNT]).toBe("Amount");
      expect(mapping[ImportFormat.QUANTITY]).toBe("Units");
      expect(mapping[ImportFormat.UNIT_PRICE]).toBe("NAV");
      expect(mapping[ImportFormat.ACCOUNT]).toBe("Folio No");
      expect(mapping[ImportFormat.SYMBOL]).toBe("Scheme Name");
      expect(mapping[ImportFormat.ISIN]).toBe("ISIN");
    });

    it("resolves Indian mutual fund transaction descriptions to canonical activity types", () => {
      // Purchases
      expect(findMappedActivityType("PURCHASE", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("SIP", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("ADDITIONAL PURCHASE", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("SWITCH IN", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("SYSTEMATIC INVESTMENT", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);
      expect(findMappedActivityType("REINVESTMENT", camsCasTemplate.activityMappings)).toBe(ActivityType.BUY);

      // Redemptions / Withdrawals
      expect(findMappedActivityType("REDEMPTION", camsCasTemplate.activityMappings)).toBe(ActivityType.SELL);
      expect(findMappedActivityType("SWITCH OUT", camsCasTemplate.activityMappings)).toBe(ActivityType.SELL);
      expect(findMappedActivityType("SWP", camsCasTemplate.activityMappings)).toBe(ActivityType.SELL);
      expect(findMappedActivityType("SYSTEMATIC WITHDRAWAL", camsCasTemplate.activityMappings)).toBe(ActivityType.SELL);

      // Dividends
      expect(findMappedActivityType("DIVIDEND PAYOUT", camsCasTemplate.activityMappings)).toBe(ActivityType.DIVIDEND);
      expect(findMappedActivityType("DIVIDEND REINVESTMENT", camsCasTemplate.activityMappings)).toBe(ActivityType.DIVIDEND);

      // Taxes & Charges
      expect(findMappedActivityType("STAMP DUTY", camsCasTemplate.activityMappings)).toBe(ActivityType.TAX);
      expect(findMappedActivityType("STT", camsCasTemplate.activityMappings)).toBe(ActivityType.TAX);
      expect(findMappedActivityType("TDS", camsCasTemplate.activityMappings)).toBe(ActivityType.TAX);
      expect(findMappedActivityType("TRANSACTION CHARGES", camsCasTemplate.activityMappings)).toBe(ActivityType.FEE);
    });
  });
});
