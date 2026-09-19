import { getIndianTaxSummary } from "@/adapters";
import { AccountScopeSelector } from "@/components/account-filter-selector";
import { useAccountScopeStore } from "@/lib/account-scope-store";
import { QueryKeys } from "@/lib/query-keys";
import type {
  IndianAssetTaxClassification,
  IndianCapitalGainCalculation,
  IndianTaxYearSummary,
} from "@/lib/types";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
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
import { AmountDisplay, AnimatedToggleGroup, Progress } from "@wealthfolio/ui";
import { useQuery } from "@tanstack/react-query";
import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DualRegimeOptimizer } from "./components/dual-regime-optimizer";

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
    case "debt_mf_post_2023":
      return "Debt MF (post-2023)";
    case "unlisted_debenture_post_2024":
      return "Unlisted Deb (post-2024)";
    case "gold_sgb":
      return "Gold / SGB";
    case "other_capital_asset":
    default:
      return "Other";
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
  const limit = 125_000;
  const percentUsed = Math.min(100, Math.round((gross / limit) * 100));
  const remainingExemption = Math.max(0, limit - gross);

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium">Section 112A — LTCG & Tax Harvesting</CardTitle>
          <Badge variant={gross > limit ? "destructive" : "outline"} className="text-[11px]">
            {gross >= limit ? "Exemption Exhausted" : `₹${Math.round(remainingExemption).toLocaleString("en-IN")} buffer left`}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1.5">
          <div className="flex justify-between text-xs">
            <span className="text-muted-foreground">Exemption Threshold Used (₹1.25L)</span>
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
            <span>Taxable LTCG (@ 12.5%)</span>
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
    { label: "STCG — Slab Rate", value: parseFloat(summary.stcgSlabTotal) || 0 },
    {
      label: "LTCG — Sec 112A (Pre Budget 2024, @ 10%)",
      value: parseFloat(summary.ltcg112aPreBudget) || 0,
    },
    {
      label: "LTCG — Sec 112A (Post Budget 2024, @ 12.5%)",
      value: parseFloat(summary.ltcg112aPostBudget) || 0,
    },
    { label: "LTCG — Other (@ 20% with indexation)", value: parseFloat(summary.ltcgOtherTotal) || 0 },
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

// ── Carry-Forward Loss Card ───────────────────────────────────────────────────

function CarryForwardCard({ summary }: { summary: IndianTaxYearSummary }) {
  const stcl = parseFloat(summary.stclCarriedForward) || 0;
  const ltcl = parseFloat(summary.ltclCarriedForward) || 0;

  if (stcl === 0 && ltcl === 0) return null;

  return (
    <Card className="border-destructive/20 bg-destructive/5">
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm font-medium">
          <Icons.AlertTriangle className="h-4 w-4 text-orange-500" />
          Carry-Forward Capital Losses
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-2 text-sm">
          {stcl < 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">Short-Term Capital Loss</span>
              <span className="text-destructive">
                <AmountDisplay value={Math.abs(stcl)} currency="INR" isHidden={false} />
              </span>
            </div>
          )}
          {ltcl < 0 && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">Long-Term Capital Loss</span>
              <span className="text-destructive">
                <AmountDisplay value={Math.abs(ltcl)} currency="INR" isHidden={false} />
              </span>
            </div>
          )}
          <p className="text-muted-foreground pt-1 text-xs">
            These losses may be carried forward for up to 8 assessment years.
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
                <TableHead className="text-right">Cost Basis</TableHead>
                <TableHead className="text-right">Proceeds</TableHead>
                <TableHead className="text-right">Gross Gain</TableHead>
                <TableHead className="text-right">Taxable Gain</TableHead>
                <TableHead className="text-right">Rate</TableHead>
                <TableHead className="text-right">Est. Tax</TableHead>
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
                    <TableCell className="font-mono text-xs">{calc.assetId}</TableCell>
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
                        {calc.holdingPeriod === "long_term" ? "LT" : "ST"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right text-xs">
                      <AmountDisplay
                        value={parseFloat(calc.deemedCost) || 0}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                    <TableCell className="text-right text-xs">
                      <AmountDisplay
                        value={parseFloat(calc.proceeds) || 0}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                    <TableCell
                      className={`text-right text-xs font-medium ${isLoss ? "text-destructive" : "text-success"}`}
                    >
                      <AmountDisplay value={grossGain} currency="INR" isHidden={false} />
                    </TableCell>
                    <TableCell className="text-right text-xs">
                      <AmountDisplay value={taxableGain} currency="INR" isHidden={false} />
                    </TableCell>
                    <TableCell className="text-right text-xs">{(rate * 100).toFixed(1)}%</TableCell>
                    <TableCell className="text-right text-xs">
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

export default function TaxPage() {
  const { t } = useTranslation();
  const currentFY = useMemo(() => getCurrentFY(), []);
  const [selectedFY, setSelectedFY] = useState(String(currentFY));

  const accountFilter = useAccountScopeStore((state) => state.scope);
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

  const filters = (
    <>
      <div className="pointer-events-auto fixed right-2 top-4 z-20 hidden items-center gap-2 md:flex lg:right-4">
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

  const stcgSlabTotal = parseFloat(summary.stcgSlabTotal) || 0;

  if (!hasData) {
    return (
      <>
        {filters}
        <div className="space-y-6">
          <EmptyPlaceholder
            className="mx-auto flex max-w-[420px] items-center justify-center pt-8 pb-4"
            icon={<Icons.FileText className="h-10 w-10" />}
            title="No disposals in this period"
            description={`No capital gains or losses recorded for ${fyLabel(parseInt(selectedFY))}.`}
          />

          {/* Dual-Regime Optimizer */}
          <DualRegimeOptimizer stcgSlabTotal={0} />
        </div>
      </>
    );
  }

  return (
    <>
      {filters}
      <div className="space-y-6">
        {/* Summary cards */}
        <div className="grid gap-4 md:grid-cols-4">
          <MetricCard
            title="Total STCG"
            value={summary.stcgTotal}
            subtitle="Short-Term Capital Gains"
            icon={<Icons.TrendingUp className="text-muted-foreground h-4 w-4" />}
            accent={stcgTotal >= 0 ? "border-blue-500/10 bg-blue-500/5" : "border-destructive/10 bg-destructive/5"}
          />
          <MetricCard
            title="Total LTCG"
            value={summary.ltcgTotal}
            subtitle="Long-Term Capital Gains"
            icon={<Icons.TrendingUp className="text-muted-foreground h-4 w-4" />}
            accent={ltcgTotal >= 0 ? "border-green-500/10 bg-green-500/5" : "border-destructive/10 bg-destructive/5"}
          />
          <MetricCard
            title="Est. STCG Tax"
            value={summary.estimatedStcgTax}
            subtitle="Estimated tax on STCG"
            icon={<Icons.Receipt className="text-muted-foreground h-4 w-4" />}
            accent={estStcgTax > 0 ? "border-orange-500/10 bg-orange-500/5" : undefined}
          />
          <MetricCard
            title="Est. LTCG Tax"
            value={summary.estimatedLtcgTax}
            subtitle="Estimated tax on LTCG"
            icon={<Icons.Receipt className="text-muted-foreground h-4 w-4" />}
            accent={estLtcgTax > 0 ? "border-orange-500/10 bg-orange-500/5" : undefined}
          />
        </div>

        {/* Detail cards */}
        <div className="grid gap-4 md:grid-cols-2">
          <Sec112ACard summary={summary} />
          <BudgetBreakdownCard summary={summary} />
        </div>

        {/* Carry-forward losses */}
        <CarryForwardCard summary={summary} />

        {/* Dual-Regime Optimizer */}
        <DualRegimeOptimizer stcgSlabTotal={stcgSlabTotal} />

        {/* Disposal-level table */}
        <DisposalTable calculations={summary.calculations} />

        <p className="text-muted-foreground text-xs">
          * Tax estimates are indicative and based on the rates applicable to each gain bucket.
          Consult a chartered accountant before filing your ITR.
        </p>
      </div>
    </>
  );
}
