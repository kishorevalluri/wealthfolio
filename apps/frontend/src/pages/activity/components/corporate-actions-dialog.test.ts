import { describe, expect, it } from "vitest";
import { ActivityType } from "@/lib/constants";
import type { CorporateActionProposal } from "@/lib/types";
import { proposalToActivity } from "./corporate-actions-dialog";

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

  it("adds nothing for an action that needs review", () => {
    expect(proposalToActivity({ ...base, kind: "REVIEW", bonusQuantity: null })).toBeNull();
  });
});
