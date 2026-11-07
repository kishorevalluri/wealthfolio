import { describe, expect, it } from "vitest";
import { applyIndianTax, extractIndianTax } from "./asset-indian-tax";

describe("extractIndianTax", () => {
  it("returns automatic and no price when nothing is stored", () => {
    expect(extractIndianTax(null)).toEqual({
      indianTaxClassification: "",
      fmv2018: null,
    });
  });

  it("reads the canonical keys", () => {
    expect(
      extractIndianTax({
        taxClassification: "debt_mf",
        fmv20180131: "1250.50",
      }),
    ).toEqual({
      indianTaxClassification: "debt_mf",
      fmv2018: 1250.5,
    });
  });

  it("reads legacy keys and spellings the Rust reader accepts", () => {
    expect(
      extractIndianTax({
        assetTaxClassification: "debt_mf_post_2023",
        fmv2018: 99,
      }),
    ).toEqual({ indianTaxClassification: "debt_mf", fmv2018: 99 });
  });

  it("treats an unknown classification as automatic", () => {
    expect(extractIndianTax({ taxClassification: "crypto" }).indianTaxClassification).toBe("");
  });
});

describe("applyIndianTax", () => {
  it("writes canonical keys and keeps other metadata", () => {
    const next = applyIndianTax(
      { identifiers: { isin: "INE002A01018" } },
      { indianTaxClassification: "equity_listed", fmv2018: 1001.25 },
    );
    expect(next).toEqual({
      identifiers: { isin: "INE002A01018" },
      taxClassification: "equity_listed",
      fmv20180131: "1001.25",
    });
  });

  it("clears the override and legacy keys when set back to automatic", () => {
    const input = {
      taxClassification: "equity_mf",
      assetTaxClassification: "equity_mf",
      fmv_2018_01_31: "10",
      fmv2018: "10",
      other: 1,
    };
    const next = applyIndianTax(input, {
      indianTaxClassification: "",
      fmv2018: null,
    });
    expect(next).toEqual({ other: 1 });
    expect(input.taxClassification).toBe("equity_mf");
  });

  it("does not store a zero or negative price", () => {
    expect(applyIndianTax({}, { indianTaxClassification: "", fmv2018: 0 })).toEqual({});
  });
});
