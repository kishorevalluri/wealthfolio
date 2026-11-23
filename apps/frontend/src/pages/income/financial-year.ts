/** Indian financial years (April to March) as the income summary names them: "FY2025" is 2025-26. */

const FINANCIAL_YEAR = /^FY(\d{4})$/;

/** Start year of a financial-year period, or null for a calendar period. */
export function financialYearStart(period: string): number | null {
  const match = FINANCIAL_YEAR.exec(period);
  return match ? Number(match[1]) : null;
}

/** "2025-26" for the financial year starting in 2025. */
export const financialYearLabel = (start: number) =>
  `${start}-${String((start + 1) % 100).padStart(2, "0")}`;
