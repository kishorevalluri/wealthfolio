import { exportIndianClearTax, exportIndianScheduleCg, getIndianTaxSummary } from "@/adapters";
import { AccountScopeSelector } from "@/components/account-filter-selector";
import { useAccountScopeStore } from "@/lib/account-scope-store";
import { QueryKeys } from "@/lib/query-keys";
import type {
  IndianAssetTaxClassification,
  IndianCapitalGainCalculation,
  IndianTaxSection,
  IndianTaxYearSummary,
} from "@/lib/types";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@wealthfolio/ui/components/ui/card";
import { EmptyPlaceholder } from "@wealthfolio/ui/components/ui/empty-placeholder";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Skeleton } from "@wealthfolio/ui/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@wealthfolio/ui/components/ui/table";
import { toast } from "@wealthfolio/ui/components/ui/use-toast";
import { AmountDisplay, AnimatedToggleGroup, Progress } from "@wealthfolio/ui";
import { useQuery } from "@tanstack/react-query";
import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAccounts } from "@/hooks/use-accounts";
import type { Account } from "@/lib/types";
import { DualRegimeOptimizer } from "./components/dual-regime-optimizer";
import { parseSpecialRateGains } from "./indian-income-tax";

// ── Financial Year Selector ────────────────────────────────────────────────────

function getCurrentFY(): number {
  const now = new Date();
  // FY starts April 1. If current month is Jan–Mar, FY start year is previous year.
  return now.getMonth() >= 3 ? now.getFullYear() : now.getFullYear() - 1;
}

function fyLabel(startYear: number): string {
  return `FY ${startYear}-${String(startYear + 1).slice(2)}`;
}

const FY_COUNT = 5; // show 5 years of history

function buildFYOptions(currentFY: number) {
  return Array.from({ length: FY_COUNT }, (_, i) => {
    const startYear = currentFY - i;
    return { value: String(startYear), label: fyLabel(startYear) };
  });
}

// ── Classification label ───────────────────────────────────────────────────────

function classificationLabel(c: IndianAssetTaxClassification): string {
  switch (c) {
    case "equity_listed":
      return "Listed Equity";
    case "equity_mf":
      return "Equity MF";
    case "reit_invit":
      return "REIT / InvIT";
    case "debt_mf":
      return "Debt / Other MF";
    case "unlisted_debenture_post_2024":
      return "Unlisted Deb (post-2024)";
    case "gold_sgb":
      return "Gold / SGB";
    case "other_capital_asset":
    default:
      return "Other";
  }
}

function taxSectionLabel(s: IndianTaxSection): string {
  switch (s) {
    case "sec111a":
      return "111A";
    case "sec112a":
      return "112A";
    case "sec112":
      return "112";
    case "sec50aa":
      return "50AA";
    case "sec10_38":
      return "10(38)";
    case "slab_stcg":
    default:
      return "Slab";
  }
}

// ── Summary Metric Card ───────────────────────────────────────────────────────

interface MetricCardProps {
  title: string;
  value: string;
  subtitle?: string;
  icon?: React.ReactNode;
  accent?: string;
}

function MetricCard({ title, value, subtitle, icon, accent }: MetricCardProps) {
  const numericValue = parseFloat(value) || 0;
  return (
    <Card className={accent}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        {icon}
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">
          <AmountDisplay value={numericValue} currency="INR" isHidden={false} />
        </div>
        {subtitle && <p className="text-muted-foreground mt-1 text-xs">{subtitle}</p>}
      </CardContent>
    </Card>
  );
}

// ── Section 112A Detail Card ──────────────────────────────────────────────────

function Sec112ACard({ summary }: { summary: IndianTaxYearSummary }) {
  const gross = parseFloat(summary.sec112aLtcgGross) || 0;
  const exemption = parseFloat(summary.sec112aExemptionApplied) || 0;
  const taxable = parseFloat(summary.sec112aLtcgTaxable) || 0;
  // ₹1L up to FY 2023-24, ₹1.25L from FY 2024-25. The exemption applies to
  // gains left after loss set-off, so measure use against what was applied.
  const limit = summary.fyStartYear >= 2024 ? 125_000 : 100_000;
  const used = exemption + taxable;
  const percentUsed = Math.min(100, Math.round((used / limit) * 100));
  const remainingExemption = Math.max(0, limit - exemption);
  const limitLabel = limit === 125_000 ? "₹1.25L" : "₹1L";

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium">
            Section 112A — LTCG & Tax Harvesting
          </CardTitle>
          <Badge variant={used > limit ? "destructive" : "outline"} className="text-2xs">
            {used >= limit
              ? "Exemption Exhausted"
              : `₹${Math.round(remainingExemption).toLocaleString("en-IN")} buffer left`}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1.5">
          <div className="flex justify-between text-xs">
            <span className="text-muted-foreground">Exemption Threshold Used ({limitLabel})</span>
            <span className="font-medium">{percentUsed}%</span>
          </div>
          <Progress value={percentUsed} className="h-2" />
        </div>

        <div className="space-y-2 text-sm">
          <div className="flex justify-between">
            <span className="text-muted-foreground">Gross LTCG (Sec 112A)</span>
            <AmountDisplay value={gross} currency="INR" isHidden={false} />
          </div>
          <div className="flex justify-between">
            <span className="text-muted-foreground">Exemption Applied</span>
            <span className="text-success">
              − <AmountDisplay value={exemption} currency="INR" isHidden={false} />
            </span>
          </div>
          <div className="flex justify-between border-t pt-2 font-medium">
            <span>Taxable LTCG (after set-off)</span>
            <AmountDisplay value={taxable} currency="INR" isHidden={false} />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Budget 2024 Pre/Post Breakdown ────────────────────────────────────────────

function BudgetBreakdownCard({ summary }: { summary: IndianTaxYearSummary }) {
  const rows = [
    {
      label: "STCG — Sec 111A (Pre Budget 2024, @ 15%)",
      value: parseFloat(summary.stcg111aPreBudget) || 0,
    },
    {
      label: "STCG — Sec 111A (Post Budget 2024, @ 20%)",
      value: parseFloat(summary.stcg111aPostBudget) || 0,
    },
    {
      label: "STCG — Slab Rate",
      value: parseFloat(summary.stcgSlabTotal) || 0,
    },
    {
      label: "LTCG — Sec 112A (Pre Budget 2024, @ 10%)",
      value: parseFloat(summary.ltcg112aPreBudget) || 0,
    },
    {
      label: "LTCG — Sec 112A (Post Budget 2024, @ 12.5%)",
      value: parseFloat(summary.ltcg112aPostBudget) || 0,
    },
    {
      label: "LTCG — Sec 112 (20% with indexation before 23 Jul 2024, 12.5% after)",
      value: parseFloat(summary.ltcgOtherTotal) || 0,
    },
    {
      label: "LTCG — Exempt u/s 10(38) (sold before 1 Apr 2018)",
      value: parseFloat(summary.exemptLtcgSec1038) || 0,
    },
  ].filter((r) => r.value !== 0);

  if (rows.length === 0) return null;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium">Gain Breakdown by Rate Bucket</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-2 text-sm">
          {rows.map((row, i) => (
            <div key={i} className="flex justify-between">
              <span className="text-muted-foreground">{row.label}</span>
              <AmountDisplay value={row.value} currency="INR" isHidden={false} />
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

// ── Income from Other Sources (Schedule OS) ───────────────────────────────────

const DIVIDEND_PERIODS = [
  "Up to 15 Jun",
  "16 Jun – 15 Sep",
  "16 Sep – 15 Dec",
  "16 Dec – 15 Mar",
  "16 Mar – 31 Mar",
];

function OtherSourcesCard({ summary }: { summary: IndianTaxYearSummary }) {
  const os = summary.otherSources;
  const dividends = parseFloat(os?.dividendsGross ?? "") || 0;
  const interest = parseFloat(os?.interestGross ?? "") || 0;
  if (!os || (dividends === 0 && interest === 0)) return null;
  const tds = (parseFloat(os.dividendsTds) || 0) + (parseFloat(os.interestTds) || 0);

  const row = (label: string, value: number) => (
    <div key={label} className="flex justify-between">
      <span className="text-muted-foreground">{label}</span>
      <AmountDisplay value={value} currency="INR" isHidden={false} />
    </div>
  );

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium">
          Income from Other Sources (Schedule OS)
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid gap-6 text-sm md:grid-cols-2">
          <div className="space-y-2">
            {row("Dividends (before TDS)", dividends)}
            {row("Interest (before TDS)", interest)}
            {row("TDS recorded", tds)}
          </div>
          {dividends > 0 && (
            <div className="space-y-2">
              <p className="text-muted-foreground text-xs">Dividends by period</p>
              {os.dividendsByPeriod.map((value, i) =>
                row(DIVIDEND_PERIODS[i] ?? "", parseFloat(value) || 0),
              )}
            </div>
          )}
        </div>
        <p className="text-muted-foreground pt-3 text-xs">
          From the dividend and interest activities recorded here; tax in their Tax field counts as
          TDS. Check the totals against your AIS and Form 26AS, and add them to other income in the
          regime comparison below.
        </p>
      </CardContent>
    </Card>
  );
}

// ── Carry-Forward Loss Card ───────────────────────────────────────────────────

function CarryForwardCard({ summary }: { summary: IndianTaxYearSummary }) {
  const stcl = parseFloat(summary.stclCarriedForward) || 0;
  const ltcl = parseFloat(summary.ltclCarriedForward) || 0;
  const stclSetOff = parseFloat(summary.stclBroughtForwardSetOff ?? "") || 0;
  const ltclSetOff = parseFloat(summary.ltclBroughtForwardSetOff ?? "") || 0;

  if (stcl === 0 && ltcl === 0 && stclSetOff === 0 && ltclSetOff === 0) return null;

  return (
    <Card className="border-warning-subtle-border bg-warning-subtle">
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm font-medium">
          <Icons.AlertTriangle className="text-warning h-4 w-4" />
          Capital Losses Carried Forward
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-2 text-sm">
          {stcl > 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">Short-term loss to carry forward</span>
              <span className="text-loss">
                <AmountDisplay value={stcl} currency="INR" isHidden={false} />
              </span>
            </div>
          )}
          {ltcl > 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">Long-term loss to carry forward</span>
              <span className="text-loss">
                <AmountDisplay value={ltcl} currency="INR" isHidden={false} />
              </span>
            </div>
          )}
          {stclSetOff > 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">
                Earlier short-term loss set off this year
              </span>
              <AmountDisplay value={stclSetOff} currency="INR" isHidden={false} />
            </div>
          )}
          {ltclSetOff > 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">
                Earlier long-term loss set off this year
              </span>
              <AmountDisplay value={ltclSetOff} currency="INR" isHidden={false} />
            </div>
          )}
          <p className="text-muted-foreground pt-1 text-xs">
            Losses are carried forward for up to 8 years. Earlier losses come from the sales
            recorded here for earlier years; check them against Schedule CFL of your last return.
          </p>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Disposal Table ─────────────────────────────────────────────────────────────

function DisposalTable({ calculations }: { calculations: IndianCapitalGainCalculation[] }) {
  if (calculations.length === 0) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm font-medium">
          Disposal Details ({calculations.length} transactions)
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Asset</TableHead>
                <TableHead>Classification</TableHead>
                <TableHead>Acquired</TableHead>
                <TableHead>Disposed</TableHead>
                <TableHead>Period</TableHead>
                <TableHead className="text-end">Cost Basis</TableHead>
                <TableHead className="text-end">Proceeds</TableHead>
                <TableHead className="text-end">Gross Gain</TableHead>
                <TableHead className="text-end">Taxable Gain</TableHead>
                <TableHead className="text-end">Rate</TableHead>
                <TableHead className="text-end">Est. Tax</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {calculations.map((calc) => {
                const grossGain = parseFloat(calc.grossGain) || 0;
                const taxableGain = parseFloat(calc.taxableGain) || 0;
                const estTax = parseFloat(calc.estimatedTax) || 0;
                const rate = parseFloat(calc.applicableTaxRate) || 0;
                const isLoss = grossGain < 0;

                return (
                  <TableRow key={`${calc.disposalId}-${calc.lotId}`}>
                    <TableCell className="text-xs">
                      <div className="flex flex-col">
                        <span className="font-medium">
                          {calc.assetName || calc.assetSymbol || calc.assetId}
                        </span>
                        {(calc.assetSymbol || calc.isin) && (
                          <span className="text-muted-foreground text-2xs">
                            {[calc.assetSymbol, calc.isin].filter(Boolean).join(" • ")}
                          </span>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline" className="text-xs">
                        {classificationLabel(calc.classification)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-xs">{calc.acquisitionDate.slice(0, 10)}</TableCell>
                    <TableCell className="text-xs">{calc.disposalDate.slice(0, 10)}</TableCell>
                    <TableCell>
                      <Badge
                        variant={calc.holdingPeriod === "long_term" ? "secondary" : "outline"}
                        className="text-xs"
                      >
                        {calc.holdingPeriod === "long_term" ? "LT" : "ST"} ·{" "}
                        {taxSectionLabel(calc.taxSection)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-end text-xs">
                      <AmountDisplay
                        value={parseFloat(calc.deemedCost) || 0}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                    <TableCell className="text-end text-xs">
                      <AmountDisplay
                        value={parseFloat(calc.proceeds) || 0}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                    <TableCell
                      className={`text-end text-xs font-medium ${isLoss ? "text-loss" : "text-gain"}`}
                    >
                      <AmountDisplay value={grossGain} currency="INR" isHidden={false} />
                    </TableCell>
                    <TableCell className="text-end text-xs">
                      <AmountDisplay value={taxableGain} currency="INR" isHidden={false} />
                    </TableCell>
                    <TableCell className="text-end text-xs">{(rate * 100).toFixed(1)}%</TableCell>
                    <TableCell className="text-end text-xs">
                      <AmountDisplay value={estTax} currency="INR" isHidden={false} />
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Loading Skeleton ──────────────────────────────────────────────────────────

function TaxPageSkeleton() {
  return (
    <div className="space-y-6">
      <div className="grid gap-4 md:grid-cols-4">
        {[...Array(4)].map((_, i) => (
          <Card key={i}>
            <CardHeader className="pb-2">
              <Skeleton className="h-4 w-24" />
            </CardHeader>
            <CardContent>
              <Skeleton className="h-8 w-36" />
              <Skeleton className="mt-2 h-3 w-24" />
            </CardContent>
          </Card>
        ))}
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        {[...Array(2)].map((_, i) => (
          <Card key={i}>
            <CardHeader>
              <Skeleton className="h-5 w-40" />
            </CardHeader>
            <CardContent className="space-y-2">
              {[...Array(4)].map((_, j) => (
                <div key={j} className="flex justify-between">
                  <Skeleton className="h-4 w-48" />
                  <Skeleton className="h-4 w-24" />
                </div>
              ))}
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────────────

/**
 * Holdings-mode accounts record quantity and average cost only, with no
 * sales, so they never have capital gains here.
 */
function HoldingsModeNotice({ accounts }: { accounts: Account[] }) {
  if (accounts.length === 0) return null;
  return (
    <div className="border-info-subtle-border bg-info-subtle text-info-subtle-foreground flex items-start gap-3 rounded-lg border p-3 text-xs">
      <Icons.Info className="mt-0.5 h-4 w-4 shrink-0" />
      <p>
        Not included: {accounts.map((a) => a.name).join(", ")}.{" "}
        {accounts.length === 1 ? "This account uses" : "These accounts use"} holdings mode, which
        records holdings but not sales. To report capital gains, switch an account you sell from to
        transactions mode and import its trades (broker tradebook or CDSL statement).
      </p>
    </div>
  );
}

export default function TaxPage() {
  const { t } = useTranslation();
  const currentFY = useMemo(() => getCurrentFY(), []);
  const [selectedFY, setSelectedFY] = useState(String(currentFY));
  const [isExporting, setIsExporting] = useState(false);
  const [isExportingClearTax, setIsExportingClearTax] = useState(false);

  const accountFilter = useAccountScopeStore((state) => state.scope);
  const { accounts } = useAccounts();
  const holdingsAccounts = useMemo(
    () => accounts.filter((a) => a.trackingMode === "HOLDINGS"),
    [accounts],
  );
  const setAccountScope = useAccountScopeStore((state) => state.setScope);

  const fyOptions = useMemo(() => buildFYOptions(currentFY), [currentFY]);

  const {
    data: summary,
    isLoading,
    error,
  } = useQuery<IndianTaxYearSummary, Error>({
    queryKey: QueryKeys.indianTaxSummary(accountFilter, parseInt(selectedFY)),
    queryFn: () => getIndianTaxSummary(accountFilter, parseInt(selectedFY)),
  });

  const handleExportScheduleCg = async () => {
    try {
      setIsExporting(true);
      const result = await exportIndianScheduleCg(accountFilter, parseInt(selectedFY));
      if (result.status === "saved") {
        toast({
          title: "Schedule CG Exported",
          description: result.filename
            ? `Successfully saved ${result.filename}`
            : "Schedule CG CSV has been downloaded.",
          variant: "success",
        });
      } else if (result.status === "empty") {
        toast({
          title: "No Data to Export",
          description:
            "No capital gains or disposal transactions found for the selected financial year.",
        });
      }
    } catch (err) {
      toast({
        title: "Export Failed",
        description: err instanceof Error ? err.message : "Failed to export Schedule CG CSV.",
        variant: "destructive",
      });
    } finally {
      setIsExporting(false);
    }
  };

  // ClearTax takes at most 5,000 rows per file, so a big year comes in parts.
  const handleExportClearTax = async () => {
    const parts = Math.max(1, summary?.cleartaxFiles ?? 1);
    const fy = parseInt(selectedFY);
    const saved: string[] = [];
    try {
      setIsExportingClearTax(true);
      for (let part = 0; part < parts; part++) {
        const result = await exportIndianClearTax(accountFilter, fy, part);
        if (result.status === "empty") {
          toast({
            title: "Nothing for ClearTax",
            description: "No share or mutual fund sales in this financial year.",
          });
          return;
        }
        if (result.status !== "saved") return;
        if (result.filename) saved.push(result.filename);
      }
      toast({
        title: "ClearTax file exported",
        description:
          parts > 1
            ? `Saved ${saved.length} files. Upload each one to ClearTax.`
            : `Saved ${saved[0] ?? "the ClearTax workbook"}. Upload it under Capital Gains in ClearTax.`,
        variant: "success",
      });
    } catch (err) {
      toast({
        title: "Export Failed",
        description: err instanceof Error ? err.message : "Failed to export the ClearTax file.",
        variant: "destructive",
      });
    } finally {
      setIsExportingClearTax(false);
    }
  };

  const clearTaxButton = (
    <Button
      size="sm"
      onClick={handleExportClearTax}
      disabled={isExportingClearTax || isLoading || !summary?.cleartaxFiles}
      className="gap-1.5 text-xs font-medium"
    >
      {isExportingClearTax ? (
        <Icons.Loader className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <Icons.Download className="h-3.5 w-3.5" />
      )}
      <span>Export for ClearTax</span>
    </Button>
  );

  const exportButton = (
    <Button
      variant="outline"
      size="sm"
      onClick={handleExportScheduleCg}
      disabled={isExporting || isLoading || !summary?.calculations?.length}
      className="gap-1.5 text-xs font-medium"
    >
      {isExporting ? (
        <Icons.Loader className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <Icons.Download className="h-3.5 w-3.5" />
      )}
      <span>Schedule CG (CSV)</span>
    </Button>
  );

  const filters = (
    <>
      <div className="pointer-events-auto fixed end-2 top-4 z-20 hidden items-center gap-2 md:flex lg:end-4">
        {clearTaxButton}
        {exportButton}
        <AccountScopeSelector value={accountFilter} onChange={setAccountScope} />
        <AnimatedToggleGroup
          variant="secondary"
          size="sm"
          items={fyOptions}
          value={selectedFY}
          onValueChange={(v) => v && setSelectedFY(v)}
        />
      </div>
      <div className="mb-6 flex flex-wrap items-center justify-end gap-2 md:hidden">
        <AnimatedToggleGroup
          variant="secondary"
          size="xs"
          items={fyOptions}
          value={selectedFY}
          onValueChange={(v) => v && setSelectedFY(v)}
        />
        <AccountScopeSelector value={accountFilter} onChange={setAccountScope} />
        {clearTaxButton}
        {exportButton}
      </div>
    </>
  );

  if (isLoading) {
    return (
      <>
        {filters}
        <TaxPageSkeleton />
      </>
    );
  }

  if (error || !summary) {
    return (
      <>
        {filters}
        <div className="p-4 md:p-6">
          <div className="text-destructive text-sm">
            {t("common:error.generic", {
              defaultValue: "Failed to load tax summary: {{message}}",
              message: error?.message ?? "unknown error",
            })}
          </div>
        </div>
      </>
    );
  }

  const stcgTotal = parseFloat(summary.stcgTotal) || 0;
  const ltcgTotal = parseFloat(summary.ltcgTotal) || 0;
  const estStcgTax = parseFloat(summary.estimatedStcgTax) || 0;
  const estLtcgTax = parseFloat(summary.estimatedLtcgTax) || 0;
  const hasData = summary.calculations.length > 0;

  const stcgSlabNet = parseFloat(summary.stcgSlabNet) || 0;

  if (!hasData) {
    return (
      <>
        {filters}
        <div className="space-y-6">
          <HoldingsModeNotice accounts={holdingsAccounts} />
          <EmptyPlaceholder
            className="mx-auto flex max-w-105 items-center justify-center pt-8 pb-4"
            icon={<Icons.FileText className="h-10 w-10" />}
            title="No disposals in this period"
            description={`No capital gains or losses recorded for ${fyLabel(parseInt(selectedFY))}.`}
          />

          <CarryForwardCard summary={summary} />
          <OtherSourcesCard summary={summary} />

          {/* Dual-Regime Optimizer */}
          <DualRegimeOptimizer fyStartYear={parseInt(selectedFY)} stcgSlabTotal={0} />
        </div>
      </>
    );
  }

  return (
    <>
      {filters}
      <div className="space-y-6">
        <HoldingsModeNotice accounts={holdingsAccounts} />
        {/* Summary cards */}
        <div className="grid gap-4 md:grid-cols-4">
          <MetricCard
            title="Total STCG"
            value={summary.stcgTotal}
            subtitle="Short-Term Capital Gains"
            icon={<Icons.TrendingUp className="text-muted-foreground h-4 w-4" />}
            accent={
              stcgTotal >= 0
                ? "border-gain-subtle-border bg-gain-subtle"
                : "border-loss-subtle-border bg-loss-subtle"
            }
          />
          <MetricCard
            title="Total LTCG"
            value={summary.ltcgTotal}
            subtitle="Long-Term Capital Gains"
            icon={<Icons.TrendingUp className="text-muted-foreground h-4 w-4" />}
            accent={
              ltcgTotal >= 0
                ? "border-gain-subtle-border bg-gain-subtle"
                : "border-loss-subtle-border bg-loss-subtle"
            }
          />
          <MetricCard
            title="Est. STCG Tax"
            value={summary.estimatedStcgTax}
            subtitle="Estimated tax on STCG"
            icon={<Icons.Receipt className="text-muted-foreground h-4 w-4" />}
            accent={estStcgTax > 0 ? "border-warning-subtle-border bg-warning-subtle" : undefined}
          />
          <MetricCard
            title="Est. LTCG Tax"
            value={summary.estimatedLtcgTax}
            subtitle={`After set-off and exemption. Total with 4% cess: ₹${Math.round(
              parseFloat(summary.estimatedTotalTax) || 0,
            ).toLocaleString("en-IN")}`}
            icon={<Icons.Receipt className="text-muted-foreground h-4 w-4" />}
            accent={estLtcgTax > 0 ? "border-warning-subtle-border bg-warning-subtle" : undefined}
          />
        </div>

        {summary.warnings.length > 0 && (
          <Card className="border-warning-subtle-border bg-warning-subtle">
            <CardHeader className="pb-2">
              <CardTitle className="flex items-center gap-2 text-sm font-medium">
                <Icons.AlertTriangle className="text-warning h-4 w-4" />
                Check before filing
              </CardTitle>
            </CardHeader>
            <CardContent>
              <ul className="list-disc space-y-1 ps-5 text-sm">
                {summary.warnings.map((w, i) => (
                  <li key={i}>{w}</li>
                ))}
              </ul>
            </CardContent>
          </Card>
        )}

        {/* Detail cards */}
        <div className="grid gap-4 md:grid-cols-2">
          <Sec112ACard summary={summary} />
          <BudgetBreakdownCard summary={summary} />
        </div>

        {/* Carry-forward losses */}
        <CarryForwardCard summary={summary} />

        <OtherSourcesCard summary={summary} />

        {/* Dual-Regime Optimizer */}
        <DualRegimeOptimizer
          fyStartYear={summary.fyStartYear}
          stcgSlabTotal={stcgSlabNet}
          specialRateGains={parseSpecialRateGains(summary.specialRateGains)}
        />

        {/* Disposal-level table */}
        <DisposalTable calculations={summary.calculations} />

        <p className="text-muted-foreground text-xs">
          * Estimates apply loss set-off (Section 70) and the Section 112A exemption, and add 4%
          cess. Section 112 sales before 23 Jul 2024 use indexed cost. Slab-rate gains, surcharge
          and the basic exemption adjustment depend on your other income, so they are worked out in
          the regime comparison above. Per-row tax is before set-off. Consult a chartered accountant
          before filing your ITR.
        </p>
      </div>
    </>
  );
}
