import type { IncomeSummary } from "@/lib/types";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { financialYearLabel, financialYearStart } from "./financial-year";
import IncomePage from "./income-page";

const mocks = vi.hoisted(() => ({ useQuery: vi.fn() }));

vi.mock("@/adapters", () => ({ getIncomeSummary: vi.fn() }));
vi.mock("@tanstack/react-query", () => ({ useQuery: mocks.useQuery }));
vi.mock("./income-history-chart", () => ({ IncomeHistoryChart: () => null }));
vi.mock("@/components/account-filter-selector", () => ({ AccountScopeSelector: () => null }));
vi.mock("@/hooks/use-balance-privacy", () => ({
  useBalancePrivacy: () => ({ isBalanceHidden: false }),
}));

function summary(period: string, byMonth: Record<string, number>): IncomeSummary {
  const totalIncome = Object.values(byMonth).reduce((sum, amount) => sum + amount, 0);
  return {
    period,
    byMonth,
    byType: { DIVIDEND: totalIncome },
    byAsset: {},
    byCurrency: { INR: totalIncome },
    byCurrencyBase: { INR: totalIncome },
    byAccount: {},
    totalIncome,
    currency: "INR",
    monthlyAverage: totalIncome / 12,
    yoyGrowth: null,
    unconvertedCurrencies: [],
  };
}

const summaries = [
  summary("ALL", { "2025-06": 1530, "2026-03": 270, "2026-06": 1836 }),
  summary("YTD", { "2026-03": 270, "2026-06": 1836 }),
  summary("LAST_YEAR", { "2025-06": 1530 }),
  summary("TWO_YEARS_AGO", {}),
  summary("FY2026", { "2026-06": 1836 }),
  summary("FY2025", { "2025-06": 1530, "2026-03": 270 }),
];

describe("IncomePage financial years", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
  });
  afterEach(() => vi.unstubAllGlobals());

  it("reads financial-year periods", () => {
    expect(financialYearStart("FY2025")).toBe(2025);
    expect(financialYearStart("LAST_YEAR")).toBeNull();
    expect(financialYearLabel(2025)).toBe("2025-26");
    expect(financialYearLabel(2099)).toBe("2099-00");
  });

  it("shows the income of a picked financial year", async () => {
    mocks.useQuery.mockReturnValue({ data: summaries, isLoading: false, error: null });
    render(<IncomePage />);
    // The desktop and mobile toolbars each have a financial-year picker.
    await userEvent.click(screen.getAllByRole("combobox", { name: "Financial year" })[0]);
    await userEvent.click(await screen.findByRole("option", { name: "FY 2025-26" }));
    expect(await screen.findByText("FY 2025-26 Income")).toBeInTheDocument();
  });
});
