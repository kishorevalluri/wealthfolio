import { describe, expect, it } from "vitest";
import {
  computeRegimeTax,
  parseSpecialRateGains,
  sectionNumber,
  surchargeRate,
} from "./indian-income-tax";

describe("computeRegimeTax", () => {
  it("gives a full rebate up to 12 lakh under the FY 2025-26 new regime", () => {
    const t = computeRegimeTax(2025, "new", 1_200_000, []);
    expect(t.slabTax).toBe(60_000);
    expect(t.rebate87a).toBe(60_000);
    expect(t.total).toBe(0);
  });

  it("limits tax just above the rebate limit to the income above it", () => {
    const t = computeRegimeTax(2025, "new", 1_210_000, []);
    expect(t.slabTax).toBe(61_500);
    expect(t.slabTax - t.rebate87a).toBe(10_000);
    expect(t.total).toBeCloseTo(10_400, 6);
  });

  it("adds special-rate gains on top of slab tax", () => {
    const t = computeRegimeTax(2024, "new", 2_000_000, [
      { section: "sec112a", rate: 0.125, amount: 200_000 },
    ]);
    expect(t.slabTax).toBe(290_000);
    expect(t.specialRateTax).toBe(25_000);
    expect(t.surcharge).toBe(0);
    expect(t.total).toBeCloseTo(327_600, 6);
  });

  it("caps surcharge on special-rate gains at 15%", () => {
    const t = computeRegimeTax(2024, "new", 15_000_000, [
      { section: "sec112a", rate: 0.125, amount: 10_000_000 },
    ]);
    expect(t.slabTax).toBe(4_190_000);
    expect(t.surcharge).toBeCloseTo(4_190_000 * 0.25 + 1_250_000 * 0.15, 6);
    expect(t.marginalRelief).toBe(0);
    expect(t.total).toBeCloseTo(6_942_000, 6);
  });

  it("gives marginal relief just over the 50 lakh surcharge threshold", () => {
    const t = computeRegimeTax(2024, "old", 5_010_000, []);
    expect(t.slabTax).toBe(1_315_500);
    expect(t.surcharge).toBeCloseTo(131_550, 6);
    expect(t.marginalRelief).toBeCloseTo(124_550, 6);
    expect(t.total).toBeCloseTo(1_375_400, 6);
  });

  it("sets unused basic exemption against special-rate gains, but no rebate", () => {
    const t = computeRegimeTax(2024, "new", 100_000, [
      { section: "sec111a", rate: 0.2, amount: 500_000 },
    ]);
    expect(t.basicExemptionAdjustment).toBe(200_000);
    expect(t.specialRateTax).toBe(60_000);
    expect(t.rebate87a).toBe(0);
    expect(t.total).toBeCloseTo(62_400, 6);
  });
});

describe("surchargeRate", () => {
  it("caps the new regime at 25%", () => {
    expect(surchargeRate(60_000_000, "old")).toBe(0.37);
    expect(surchargeRate(60_000_000, "new")).toBe(0.25);
    expect(surchargeRate(5_000_000, "new")).toBe(0);
  });
});

describe("parseSpecialRateGains", () => {
  it("parses decimal strings and drops empty rows", () => {
    expect(
      parseSpecialRateGains([
        { section: "sec112a", rate: "0.125", amount: "75000" },
        { section: "sec111a", rate: "0.20", amount: "0" },
      ]),
    ).toEqual([{ section: "sec112a", rate: 0.125, amount: 75_000 }]);
  });
});

describe("sectionNumber", () => {
  it("uses the Income-tax Act 2025 numbers from 2026-27", () => {
    expect(sectionNumber("111A", 2025)).toBe("111A");
    expect(sectionNumber("111A", 2026)).toBe("196");
    expect(sectionNumber("112A", 2026)).toBe("198");
    expect(sectionNumber("87A", 2026)).toBe("156");
    expect(sectionNumber("50AA", 2026)).toBe("50AA");
  });
});
