import { getNseCorporateActionProposals } from "@/adapters";
import { ActivityType } from "@/lib/constants";
import type { ActivityCreate, CorporateActionProposal } from "@/lib/types";
import { useQuery } from "@tanstack/react-query";
import { Spinner } from "@wealthfolio/ui";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Checkbox } from "@wealthfolio/ui/components/ui/checkbox";
import { Label } from "@wealthfolio/ui/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useActivityMutations } from "../hooks/use-activity-mutations";

export const CORPORATE_ACTIONS_QUERY_KEY = ["nse-corporate-action-proposals"];

const proposalKey = (p: CorporateActionProposal) =>
  `${p.accountId}|${p.assetId}|${p.exDate}|${p.kind}`;

/** The activity a bonus or split proposal adds; none for one to review. */
export function proposalToActivity(p: CorporateActionProposal): ActivityCreate | null {
  const base = {
    accountId: p.accountId,
    activityDate: p.activityDate,
    asset: { id: p.assetId },
    currency: "INR",
    comment: `NSE: ${p.subject}`,
  };
  if (p.kind === "BONUS" && p.bonusQuantity) {
    return {
      ...base,
      activityType: ActivityType.BUY,
      quantity: p.bonusQuantity,
      unitPrice: "0",
      amount: "0",
      fee: "0",
      tax: "0",
    };
  }
  if (p.kind === "SPLIT" && p.splitRatio) {
    return { ...base, activityType: ActivityType.SPLIT, amount: p.splitRatio };
  }
  if (p.kind === "DIVIDEND" && p.dividendAmount) {
    return { ...base, activityType: ActivityType.DIVIDEND, amount: p.dividendAmount };
  }
  return null;
}

const NO_DIVIDENDS = "none";

/** Start year of the Indian financial year (April to March) `date` falls in. */
export function financialYearOf(date: Date): number {
  return date.getMonth() >= 3 ? date.getFullYear() : date.getFullYear() - 1;
}

/** Financial years to offer as the start of dividends, newest first. */
function financialYearOptions(today = new Date()): number[] {
  const current = financialYearOf(today);
  return Array.from({ length: 9 }, (_, i) => current - i);
}

const fyLabel = (start: number) => `${start}-${String((start + 1) % 100).padStart(2, "0")}`;

function trimNumber(value?: string | null): string {
  if (!value) return "";
  const n = Number(value);
  return Number.isFinite(n) ? n.toLocaleString(undefined, { maximumFractionDigits: 4 }) : value;
}

export interface CorporateActionsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function CorporateActionsDialog({ open, onOpenChange }: CorporateActionsDialogProps) {
  const { t } = useTranslation();
  const { saveActivitiesMutation } = useActivityMutations();
  const years = useMemo(() => financialYearOptions(), []);
  // Last financial year by default: the one being filed.
  const [dividendsFrom, setDividendsFrom] = useState<string>(String(years[1]));
  const dividendsSince = dividendsFrom === NO_DIVIDENDS ? undefined : `${dividendsFrom}-04-01`;
  const query = useQuery({
    queryKey: [...CORPORATE_ACTIONS_QUERY_KEY, dividendsSince ?? NO_DIVIDENDS],
    queryFn: () => getNseCorporateActionProposals(dividendsSince),
    enabled: open,
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });
  const proposals = useMemo(() => query.data?.proposals ?? [], [query.data]);
  const addable = useMemo(() => proposals.filter((p) => p.kind !== "REVIEW"), [proposals]);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  useEffect(() => {
    setSelected(new Set(addable.map(proposalKey)));
  }, [addable]);

  const failed = query.data?.failedSymbols ?? [];
  const checked = query.data?.symbolsChecked ?? [];
  const allFailed = checked.length > 0 && failed.length === checked.length;

  const toggle = (key: string, on: boolean) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(key);
      else next.delete(key);
      return next;
    });

  const add = () => {
    const creates = addable
      .filter((p) => selected.has(proposalKey(p)))
      .map(proposalToActivity)
      .filter((a): a is ActivityCreate => a !== null);
    if (creates.length === 0) return;
    saveActivitiesMutation.mutate(
      { creates },
      {
        onSuccess: () => {
          onOpenChange(false);
        },
      },
    );
  };

  const detail = (p: CorporateActionProposal) => {
    if (p.kind === "BONUS") {
      return t("activity:corporate_actions.bonus_detail", {
        quantity: trimNumber(p.bonusQuantity),
        held: trimNumber(p.heldQuantity),
      });
    }
    if (p.kind === "SPLIT") {
      const after = Number(p.heldQuantity) * Number(p.splitRatio);
      return t("activity:corporate_actions.split_detail", {
        held: trimNumber(p.heldQuantity),
        after: trimNumber(String(after)),
      });
    }
    if (p.kind === "DIVIDEND") {
      return t("activity:corporate_actions.dividend_detail", {
        amount: trimNumber(p.dividendAmount),
        held: trimNumber(p.heldQuantity),
        rate: trimNumber(p.dividendPerShare),
      });
    }
    return t("activity:corporate_actions.review_detail");
  };

  const count = addable.filter((p) => selected.has(proposalKey(p))).length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("activity:corporate_actions.title")}</DialogTitle>
          <DialogDescription>{t("activity:corporate_actions.description")}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap items-center gap-2">
          <Label htmlFor="dividends-from" className="text-sm">
            {t("activity:corporate_actions.dividends_from")}
          </Label>
          <Select
            value={dividendsFrom}
            items={{
              [NO_DIVIDENDS]: t("activity:corporate_actions.dividends_none"),
              ...Object.fromEntries(
                years.map((y) => [
                  String(y),
                  t("activity:corporate_actions.financial_year", { year: fyLabel(y) }),
                ]),
              ),
            }}
            onValueChange={(value) => {
              if (value !== null) setDividendsFrom(value);
            }}
          >
            <SelectTrigger id="dividends-from" className="h-8 w-auto text-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_DIVIDENDS}>
                {t("activity:corporate_actions.dividends_none")}
              </SelectItem>
              {years.map((y) => (
                <SelectItem key={y} value={String(y)}>
                  {t("activity:corporate_actions.financial_year", { year: fyLabel(y) })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {query.isFetching ? (
          <div className="text-muted-foreground flex items-center gap-2 py-6 text-sm">
            <Spinner className="size-4" />
            {t("activity:corporate_actions.loading")}
          </div>
        ) : query.isError ? (
          <p className="text-destructive py-4 text-sm" role="alert">
            {String((query.error as Error)?.message ?? query.error)}
          </p>
        ) : allFailed ? (
          <p className="text-destructive py-4 text-sm" role="alert">
            {t("activity:corporate_actions.failed_all")}
          </p>
        ) : (
          <div className="space-y-3">
            {proposals.length === 0 ? (
              <p className="py-4 text-sm">{t("activity:corporate_actions.none")}</p>
            ) : (
              <ul className="divide-border divide-y rounded-md border">
                {proposals.map((p) => {
                  const key = proposalKey(p);
                  const canAdd = p.kind !== "REVIEW";
                  return (
                    <li key={key} className="flex items-start gap-3 p-3 text-sm">
                      <Checkbox
                        aria-label={`${p.symbol} ${p.subject}`}
                        className="mt-0.5"
                        disabled={!canAdd}
                        checked={canAdd && selected.has(key)}
                        onCheckedChange={(on) => toggle(key, on === true)}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-baseline gap-x-2">
                          <span className="font-medium">{p.symbol}</span>
                          <span className="text-muted-foreground">{p.accountName}</span>
                          <span className="text-muted-foreground">{p.activityDate}</span>
                          <span className="rounded bg-current/10 px-1.5 text-xs">
                            {t(`activity:corporate_actions.kind_${p.kind.toLowerCase()}`)}
                          </span>
                        </div>
                        <div>{p.subject}</div>
                        <div className="text-muted-foreground">{detail(p)}</div>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            {(query.data?.alreadyRecorded ?? 0) > 0 && (
              <p className="text-muted-foreground text-xs">
                {t("activity:corporate_actions.already_recorded", {
                  count: query.data?.alreadyRecorded ?? 0,
                })}
              </p>
            )}
            {failed.length > 0 && (
              <p className="text-muted-foreground text-xs">
                {t("activity:corporate_actions.failed_some", {
                  symbols: failed.map(([symbol]) => symbol).join(", "),
                })}
              </p>
            )}
          </div>
        )}

        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={() => query.refetch()} disabled={query.isFetching}>
            {t("activity:corporate_actions.retry")}
          </Button>
          <Button
            onClick={add}
            disabled={count === 0 || query.isFetching || saveActivitiesMutation.isPending}
          >
            {saveActivitiesMutation.isPending ? <Spinner className="me-2 size-4" /> : null}
            {t("activity:corporate_actions.add_selected", { count })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
