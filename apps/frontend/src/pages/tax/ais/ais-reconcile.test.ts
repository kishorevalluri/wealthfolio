import { describe, expect, it } from "vitest";
import type { IndianCapitalGainCalculation, IndianTaxYearSummary } from "@/lib/types";
import { parseAisCsv, parseAisDate, parseAisFile, parseAmount } from "./ais-parser";
import { nameScore, reconcileAis, reconcileDividends } from "./ais-reconcile";

function calc(overrides: Partial<IndianCapitalGainCalculation>): IndianCapitalGainCalculation {
  return {
    disposalId: "d",
    lotId: "l",
    assetId: "INFY",
    classification: "listed_equity" as IndianCapitalGainCalculation["classification"],
    taxSection: "sec112a",
    acquisitionDate: "2020-01-10",
    disposalDate: "2025-07-15",
    holdingPeriod: "long_term" as IndianCapitalGainCalculation["holdingPeriod"],
    actualCost: "10000",
    deemedCost: "10000",
    proceeds: "15000",
    grossGain: "5000",
    taxableGain: "5000",
    applicableTaxRate: "0.125",
    estimatedTax: "625",
    assetName: "Infosys Limited",
    isin: "INE009A01021",
    quantity: "10",
    salePricePerUnit: "1500",
    ...overrides,
  };
}

function summary(over: Partial<IndianTaxYearSummary>): IndianTaxYearSummary {
  return {
    fyStartYear: 2025,
    calculations: [],
    warnings: [],
    otherSources: {
      dividendsGross: "0",
      dividendsTds: "0",
      interestGross: "0",
      interestTds: "0",
      dividendsByPeriod: [],
      dividendsByAsset: [],
    },
    ...over,
  } as IndianTaxYearSummary;
}

describe("AIS parsing", () => {
  it("reads Indian dates and amounts", () => {
    expect(parseAisDate("15/07/2025")).toBe("2025-07-15");
    expect(parseAisDate("15-Jul-2025")).toBe("2025-07-15");
    expect(parseAisDate("2025-07-15T00:00:00")).toBe("2025-07-15");
    expect(parseAisDate("31/02/2025")).toBeUndefined();
    expect(parseAmount("₹ 1,23,456.70")).toBe(123456.7);
    expect(parseAmount("-")).toBeUndefined();
  });

  it("reads nested JSON, taking the code and company from the parent", () => {
    const json = JSON.stringify({
      financialYear: "FY 2025-26",
      info: [
        {
          informationCode: "TDS-194",
          informationDescription: "Dividend",
          informationSource: "INFOSYS LIMITED (BLRI03163G)",
          transactions: [
            { dateOfPaymentCredit: "30/06/2025", amountPaidCredited: "2200", taxDeducted: "220" },
          ],
        },
        {
          informationCode: "SFT-018",
          informationDescription: "Sale of securities and units of mutual fund",
          informationSource: "CDSL",
          transactions: [
            {
              dateOfSaleTransfer: "15/07/2025",
              securityName: "INFOSYS LTD",
              securityCode: "INE009A01021",
              quantity: "10",
              salePricePerUnit: "1500",
              salesConsideration: "15000",
              costOfAcquisition: "0",
            },
          ],
        },
      ],
    });
    const s = parseAisFile(json);
    expect(s.fyStartYear).toBe(2025);
    expect(s.dividends).toEqual([
      expect.objectContaining({ code: "TDS-194", date: "2025-06-30", amount: 2200, tds: 220 }),
    ]);
    expect(s.sales).toEqual([
      expect.objectContaining({
        code: "SFT-018",
        date: "2025-07-15",
        isin: "INE009A01021",
        quantity: 10,
        consideration: 15000,
        cost: 0,
        reportedBy: "CDSL",
      }),
    ]);
  });

  it("reads a CSV with a title line above its header", () => {
    const csv = [
      "SFT-018 Sale of securities and units of mutual fund",
      "Sr. No.,Date of Sale/Transfer,Security Name,Security Code,Quantity,Sales Price per unit,Sales Consideration,Cost of Acquisition",
      '1,15/07/2025,INFOSYS LTD,INE009A01021,10,1500,"15,000",',
    ].join("\n");
    const s = parseAisCsv(csv);
    expect(s.sales).toHaveLength(1);
    expect(s.sales[0].cost).toBeUndefined();
  });
});

describe("AIS reconciliation", () => {
  it("matches company names loosely", () => {
    expect(nameScore("INFOSYS LIMITED", "Infosys Ltd")).toBe(1);
    expect(nameScore("HDFC ASSET MANAGEMENT COMPANY LIMITED", "HDFCAMC")).toBeGreaterThan(0.6);
    expect(nameScore("TATA STEEL LIMITED", "TATA MOTORS LIMITED")).toBeLessThan(0.6);
  });

  it("matches a sale AIS dates on the settlement day and flags a ₹0 cost", () => {
    const result = reconcileAis(
      {
        dividends: [],
        skipped: 0,
        sales: [
          {
            code: "SFT-018",
            date: "2025-07-16",
            name: "INFOSYS LTD",
            isin: "INE009A01021",
            quantity: 10,
            consideration: 15000,
            cost: 0,
          },
        ],
      },
      summary({ calculations: [calc({})] }),
    );
    const [scrip] = result.scrips;
    expect(scrip.status).toBe("match");
    expect(scrip.rows[0].aisDate).toBe("2025-07-16");
    expect(scrip.rows[0].hints.join(" ")).not.toMatch(/day\(s\) later/);
    expect(scrip.rows[0].hints.join(" ")).toMatch(/cost as ₹0/);
  });

  it("counts a sale two reporters both listed once", () => {
    const sale = {
      code: "SFT-018",
      date: "2025-07-15",
      name: "INFOSYS LTD",
      isin: "INE009A01021",
      quantity: 10,
      consideration: 15000,
    };
    const result = reconcileAis(
      {
        dividends: [],
        skipped: 0,
        sales: [
          { ...sale, reportedBy: "CDSL" },
          { ...sale, reportedBy: "ZERODHA" },
        ],
      },
      summary({ calculations: [calc({})] }),
    );
    expect(result.scrips[0].status).toBe("match");
    expect(result.scrips[0].rows[0].ais?.duplicated).toBe(true);
  });

  it("shows sales only one side has", () => {
    const result = reconcileAis(
      {
        dividends: [],
        skipped: 0,
        sales: [{ code: "SFT-018", date: "2025-09-01", name: "TCS LTD", consideration: 40000 }],
      },
      summary({ calculations: [calc({})] }),
    );
    expect(result.scrips.map((s) => s.status).sort()).toEqual(["ais-only", "app-only"]);
  });

  it("does not add SFT-015 and TDS-194 for the same dividends", () => {
    const rows = reconcileDividends(
      [
        { code: "SFT-015", company: "INFOSYS LIMITED", amount: 2200, tds: 0 },
        { code: "TDS-194", company: "INFOSYS LIMITED", amount: 2200, tds: 220 },
        { code: "SFT-015", company: "ITC LIMITED", amount: 1000, tds: 0 },
      ],
      [
        {
          assetId: "INFY",
          name: "Infosys Ltd",
          gross: 2200,
          tds: 220,
          payments: [{ date: "2025-06-30", gross: 2200, tds: 220 }],
        },
        { assetId: "ITC", name: "ITC Ltd", gross: 500, tds: 0, payments: [] },
        { assetId: "COAL", name: "Coal India Ltd", gross: 300, tds: 0, payments: [] },
      ],
    );
    const by = Object.fromEntries(rows.map((r) => [r.name, r]));
    expect(by["Infosys Ltd"].status).toBe("match");
    expect(by["ITC Ltd"].status).toBe("different");
    expect(by["ITC Ltd"].hints.join(" ")).toMatch(/exactly double/);
    expect(by["Coal India Ltd"].status).toBe("app-only");
  });

  it("uses a manual pairing when names differ too much", () => {
    const rows = reconcileDividends(
      [{ code: "SFT-015", company: "ITC HOTELS LIMITED", amount: 100, tds: 0 }],
      [{ assetId: "X1", name: "Something Else", gross: 100, tds: 0, payments: [] }],
      { "ITC HOTELS LIMITED": "X1" },
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].status).toBe("match");
  });

  it("folds a renamed company's two AIS names into one row", () => {
    const rows = reconcileDividends(
      [
        { code: "SFT-015", company: "JUBILANT LIFE SCIENCES LIMITED", amount: 250, tds: 0 },
        { code: "TDS-194", company: "JUBILANT PHARMOVA LIMITED", amount: 250, tds: 0 },
      ],
      [{ assetId: "JUBLPHARMA", name: "Jubilant Pharmova Ltd", gross: 250, tds: 0, payments: [] }],
      { "JUBILANT LIFE SCIENCES LIMITED": "JUBLPHARMA" },
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].status).toBe("match");
    expect(rows[0].aisCompanies).toHaveLength(2);
    expect(rows[0].hints[0]).toMatch(/2 names/);
  });

  it("reads the portal's AIS utility layout", () => {
    const label = (field: string) => ({ field, name: field });
    const json = JSON.stringify({
      header: { columnLabel: ["Financial Year"], columnData: ["2025-26"] },
      partB: {
        sections: [
          {
            sectionKey: "sft",
            elements: [
              {
                title: "Dividend",
                l2: {
                  columnData: [
                    ["Dividend", "SFT-015", "Dividend income", "ACME LIMITED (AAAAA0000A.AZ000)"],
                  ],
                },
                l1: {
                  columnLabel: ["tsnId", "reportedOn", "amtPaid", "status"].map(label),
                  columnData: [
                    ["1", "29/05/2026", "1,200.00", "Active"],
                    ["2", "29/05/2026", "100.00", "Inactive"],
                  ],
                },
              },
              {
                title: "Sale of securities and units of mutual fund",
                l2: {
                  columnData: [
                    [
                      "Sale",
                      "SFT-17-LES(M)",
                      "Sale of listed equity share",
                      "CENTRAL DEPOSITORY SERVICES(I) LIMITED (AAAAA0000A)",
                    ],
                  ],
                },
                l1: {
                  columnLabel: [
                    "tsnId",
                    "transferDate",
                    "securityName",
                    "debitType",
                    "creditType",
                    "assetType",
                    "quantity",
                    "sellPricePerUnit",
                    "salesConsideration",
                    "costOfAcquisition",
                    "status",
                  ].map(label),
                  columnData: [
                    [
                      "3",
                      "10/02/2026",
                      "INFOSYS LIMITED # EQUITY SHARES(INE009A01021)",
                      "Market",
                      "Market",
                      "Long term",
                      "1.00",
                      "1,500.00",
                      "1,500.00",
                      "1,000.00",
                      "Active",
                    ],
                  ],
                },
              },
            ],
          },
        ],
      },
    });
    const s = parseAisFile(json);
    expect(s.fyStartYear).toBe(2025);
    expect(s.skipped).toBe(1);
    expect(s.dividends).toEqual([
      expect.objectContaining({
        company: "ACME LIMITED",
        amount: 1200,
        reportedOn: "2026-05-29",
      }),
    ]);
    expect(s.dividends[0].date).toBeUndefined();
    expect(s.sales[0]).toEqual(
      expect.objectContaining({
        name: "INFOSYS LIMITED",
        isin: "INE009A01021",
        date: "2026-02-10",
        quantity: 1,
        consideration: 1500,
        cost: 1000,
        term: "Long term",
        reportedBy: "CENTRAL DEPOSITORY SERVICES(I) LIMITED",
      }),
    );
  });
});
