import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { addDays, format, parseISO } from "date-fns";
import { getHoldingsList, searchActivities } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { DraftActivity } from "../context";
import { findBalanceMismatches, type StatementBalance } from "../utils/statement-balances";
import { ImportAlert } from "./import-alert";

interface StatementBalanceCheckProps {
  accountId: string;
  balances: StatementBalance[];
  drafts: DraftActivity[];
}

/**
 * Checks the import against the closing balances on a depository statement:
 * what the account will hold on the statement date, security by security.
 */
export function StatementBalanceCheck({ accountId, balances, drafts }: StatementBalanceCheckProps) {
  const { t } = useTranslation();
  const statementDate = balances[0]?.date ?? "";
  const enabled = !!accountId && balances.length > 0 && /^\d{4}-\d{2}-\d{2}$/.test(statementDate);

  const holdingsQuery = useQuery({
    queryKey: [QueryKeys.HOLDINGS, { type: "account", accountId }, { includeClosed: true }],
    queryFn: () => getHoldingsList({ type: "account", accountId }, { includeClosed: true }),
    enabled,
  });
  const laterQuery = useQuery({
    queryKey: [QueryKeys.ACTIVITIES, "after-statement", accountId, statementDate],
    queryFn: async () =>
      (
        await searchActivities(
          0,
          Number.MAX_SAFE_INTEGER,
          {
            accountIds: [accountId],
            activityTypes: ["BUY", "SELL", "TRANSFER_IN", "TRANSFER_OUT"],
            dateFrom: format(addDays(parseISO(statementDate), 1), "yyyy-MM-dd"),
          },
          "",
        )
      ).data,
    enabled,
  });

  const mismatches = useMemo(() => {
    if (!holdingsQuery.data || !laterQuery.data) return null;
    return findBalanceMismatches({
      balances,
      holdings: holdingsQuery.data.map((h) => ({
        assetId: h.instrument?.id,
        isin: h.instrument?.isin,
        quantity: h.quantity,
      })),
      laterActivities: laterQuery.data.map((a) => ({
        assetId: a.assetId,
        activityType: a.activityType,
        quantity: a.quantity,
      })),
      drafts,
    });
  }, [balances, drafts, holdingsQuery.data, laterQuery.data]);

  if (!enabled || !mismatches) return null;

  if (mismatches.length === 0) {
    return (
      <ImportAlert
        variant="success"
        title={t("activity:import.review.balanceCheck.matchTitle", { date: statementDate })}
        description={t("activity:import.review.balanceCheck.matchDescription", {
          count: balances.length,
        })}
      />
    );
  }

  return (
    <ImportAlert
      variant="warning"
      title={t("activity:import.review.balanceCheck.mismatchTitle", { date: statementDate })}
      description={t("activity:import.review.balanceCheck.mismatchDescription")}
    >
      <table className="mt-2 w-full text-xs">
        <thead>
          <tr className="text-muted-foreground text-start">
            <th className="py-1 text-start font-medium">ISIN</th>
            <th className="py-1 text-end font-medium">
              {t("activity:import.review.balanceCheck.statement")}
            </th>
            <th className="py-1 text-end font-medium">
              {t("activity:import.review.balanceCheck.account")}
            </th>
          </tr>
        </thead>
        <tbody>
          {mismatches.map((m) => (
            <tr key={m.isin}>
              <td className="py-1">
                <span className="font-mono">{m.isin}</span>
                {m.name && <span className="text-muted-foreground ms-2">{m.name}</span>}
              </td>
              <td className="py-1 text-end tabular-nums">{Number(m.statement.toFixed(3))}</td>
              <td className="py-1 text-end tabular-nums">{Number(m.account.toFixed(3))}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </ImportAlert>
  );
}
