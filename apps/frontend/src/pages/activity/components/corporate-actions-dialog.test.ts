import { describe, expect, it } from "vitest";
import { ActivityType } from "@/lib/constants";
import type { CorporateActionProposal } from "@/lib/types";
import { financialYearOf, proposalToActivity } from "./corporate-actions-dialog";

const base: CorporateActionProposal = {
  kind: "BONUS",
  accountId: "acc-1",
  accountName: "Broker",
  assetId: "asset-1",
  symbol: "EXAMPLE",
  subject: "Bonus 1:1",
  exDate: "2025-11-26",
  recordDate: "2025-11-26",
  activityDate: "2025-11-26",
  heldQuantity: "17",
  bonusQuantity: "17",
};

describe("NSE corporate action proposals as activities", () => {
  it("adds a bonus as a buy at a price of 0", () => {
    expect(proposalToActivity(base)).toMatchObject({
      accountId: "acc-1",
      activityType: ActivityType.BUY,
      activityDate: "2025-11-26",
      asset: { id: "asset-1" },
      quantity: "17",
      unitPrice: "0",
      amount: "0",
    });
  });

  it("adds a split with its ratio as the amount", () => {
    const split = proposalToActivity({
      ...base,
      kind: "SPLIT",
      subject: "Face Value Split - From Rs 10/- To Rs 2/-",
      bonusQuantity: null,
      splitRatio: "5",
    });
    expect(split).toMatchObject({ activityType: ActivityType.SPLIT, amount: "5" });
    expect(split?.quantity).toBeUndefined();
  });

  it("adds a dividend with its amount before TDS", () => {
    const dividend = proposalToActivity({
      ...base,
      kind: "DIVIDEND",
      subject: "Final Dividend - Rs 90 Per Share",
      bonusQuantity: null,
      dividendPerShare: "90",
      dividendAmount: "1530",
    });
    expect(dividend).toMatchObject({
      activityType: ActivityType.DIVIDEND,
      amount: "1530",
      currency: "INR",
    });
    expect(dividend?.quantity).toBeUndefined();
  });

  it("reads the Indian financial year a date falls in", () => {
    expect(financialYearOf(new Date(2026, 2, 31))).toBe(2025);
    expect(financialYearOf(new Date(2026, 3, 1))).toBe(2026);
  });

  it("adds nothing for an action that needs review", () => {
    expect(proposalToActivity({ ...base, kind: "REVIEW", bonusQuantity: null })).toBeNull();
  });
});
