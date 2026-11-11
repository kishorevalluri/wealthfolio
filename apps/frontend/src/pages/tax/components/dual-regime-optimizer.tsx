import { AmountDisplay } from "@wealthfolio/ui";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@wealthfolio/ui/components/ui/card";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Label } from "@wealthfolio/ui/components/ui/label";
import { Switch } from "@wealthfolio/ui/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@wealthfolio/ui/components/ui/table";
import { useState } from "react";

import {
  computeRegimeTax,
  sectionNumber,
  standardDeduction,
  type RegimeTax,
  type SpecialRateGain,
} from "../indian-income-tax";

export interface DualRegimeOptimizerProps {
  /** Financial year start (2025 = FY 2025-26); picks the slabs. */
  fyStartYear: number;
  /** Slab-rate capital gains from the portfolio, after set-off */
  stcgSlabTotal?: number;
  /** Special-rate capital gains from the portfolio, after set-off and exemption */
  specialRateGains?: SpecialRateGain[];
}

export function DualRegimeOptimizer({
  fyStartYear,
  stcgSlabTotal = 0,
  specialRateGains = [],
}: DualRegimeOptimizerProps) {
  const [grossSalary, setGrossSalary] = useState<string>("1500000");
  const [otherIncome, setOtherIncome] = useState<string>("0");
  const [includePortfolioSlabGains, setIncludePortfolioSlabGains] = useState<boolean>(true);

  // Old regime deductions
  const [sec80C, setSec80C] = useState<string>("150000");
  const [sec80CCD, setSec80CCD] = useState<string>("50000");
  const [sec80D, setSec80D] = useState<string>("25000");
  const [hra, setHra] = useState<string>("0");
  const [homeLoanInterest, setHomeLoanInterest] = useState<string>("0");
  const [otherDeductions, setOtherDeductions] = useState<string>("0");

  const [showDeductions, setShowDeductions] = useState<boolean>(false);

  const STANDARD_DEDUCTION_OLD = standardDeduction(fyStartYear, "old");
  const STANDARD_DEDUCTION_NEW = standardDeduction(fyStartYear, "new");
  const fyLabel = `FY ${fyStartYear}-${String((fyStartYear + 1) % 100).padStart(2, "0")}`;

  // Parsed values
  const numSalary = Math.max(0, parseFloat(grossSalary) || 0);
  const numOther = Math.max(0, parseFloat(otherIncome) || 0);
  const additionalSlabGains = includePortfolioSlabGains ? Math.max(0, stcgSlabTotal) : 0;
  const portfolioSpecialGains = includePortfolioSlabGains ? specialRateGains : [];
  const specialIncome = portfolioSpecialGains.reduce((sum, g) => sum + g.amount, 0);
  const slabGrossIncome = numSalary + numOther + additionalSlabGains;
  const totalGrossIncome = slabGrossIncome + specialIncome;

  // Deductions calculation (Old Regime)
  const capped80C = Math.min(150_000, Math.max(0, parseFloat(sec80C) || 0));
  const capped80CCD = Math.min(50_000, Math.max(0, parseFloat(sec80CCD) || 0));
  const num80D = Math.max(0, parseFloat(sec80D) || 0);
  const numHra = Math.max(0, parseFloat(hra) || 0);
  const cappedHomeLoan = Math.min(200_000, Math.max(0, parseFloat(homeLoanInterest) || 0));
  const numOtherDeductions = Math.max(0, parseFloat(otherDeductions) || 0);

  const totalOldDeductions =
    STANDARD_DEDUCTION_OLD +
    capped80C +
    capped80CCD +
    num80D +
    numHra +
    cappedHomeLoan +
    numOtherDeductions;

  // Deductions reduce slab income only, never special-rate gains.
  const oldSlabIncome = Math.max(0, slabGrossIncome - totalOldDeductions);
  const newSlabIncome = Math.max(0, slabGrossIncome - STANDARD_DEDUCTION_NEW);

  const oldTax: RegimeTax = computeRegimeTax(
    fyStartYear,
    "old",
    oldSlabIncome,
    portfolioSpecialGains,
  );
  const newTax: RegimeTax = computeRegimeTax(
    fyStartYear,
    "new",
    newSlabIncome,
    portfolioSpecialGains,
  );

  const oldTaxableIncome = oldTax.totalIncome;
  const newTaxableIncome = newTax.totalIncome;
  const oldTotalTax = Math.round(oldTax.total);
  const newTotalTax = Math.round(newTax.total);

  const difference = Math.abs(oldTotalTax - newTotalTax);
  const recommendedRegime = newTotalTax <= oldTotalTax ? "new" : "old";
  const isBreakEven = oldTotalTax === newTotalTax;
  const hasPortfolioGains = stcgSlabTotal > 0 || specialRateGains.length > 0;

  return (
    <Card className="border-border">
      <CardHeader>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2 text-base font-semibold">
              <Icons.Receipt className="text-primary h-5 w-5" />
              Dual-Regime Tax Optimizer ({fyLabel})
            </CardTitle>
            <CardDescription className="text-xs">
              Old vs. new regime for {fyLabel}: slabs, standard deduction, Section{" "}
              {sectionNumber("87A", fyStartYear)} rebate, surcharge with marginal relief, and your
              capital gains from this portfolio.
            </CardDescription>
          </div>
          <Badge
            variant={recommendedRegime === "new" ? "default" : "secondary"}
            className="w-fit px-3 py-1 text-xs font-semibold"
          >
            {isBreakEven
              ? "Both Regimes Equal"
              : recommendedRegime === "new"
                ? `New Regime Saves ₹${difference.toLocaleString("en-IN")}`
                : `Old Regime Saves ₹${difference.toLocaleString("en-IN")}`}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* Income inputs */}
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <div className="space-y-1.5">
            <Label htmlFor="gross-salary" className="text-xs">
              Gross Annual Salary (₹)
            </Label>
            <Input
              id="gross-salary"
              type="number"
              min="0"
              value={grossSalary}
              onChange={(e) => setGrossSalary(e.target.value)}
              className="h-9 text-xs"
              placeholder="e.g. 1500000"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="other-income" className="text-xs">
              Other Income / Interest (₹)
            </Label>
            <Input
              id="other-income"
              type="number"
              min="0"
              value={otherIncome}
              onChange={(e) => setOtherIncome(e.target.value)}
              className="h-9 text-xs"
              placeholder="e.g. 50000"
            />
          </div>

          {hasPortfolioGains && (
            <div className="flex flex-col justify-end space-y-1.5">
              <div className="flex items-center justify-between rounded-md border p-2">
                <div className="space-y-0.5">
                  <span className="text-xs font-medium">Include Capital Gains</span>
                  <p className="text-muted-foreground text-2xs">
                    +₹
                    {Math.round(
                      stcgSlabTotal + specialRateGains.reduce((s, g) => s + g.amount, 0),
                    ).toLocaleString("en-IN")}{" "}
                    from portfolio
                  </p>
                </div>
                <Switch
                  checked={includePortfolioSlabGains}
                  onCheckedChange={setIncludePortfolioSlabGains}
                  size="sm"
                />
              </div>
            </div>
          )}
        </div>

        {/* Collapsible Old Regime Deductions */}
        <div className="bg-muted/20 space-y-3 rounded-lg border p-3">
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <div className="text-xs font-medium">Old Regime Deductions (Chapter VI-A & HRA)</div>
              <p className="text-muted-foreground text-2xs">
                Active deductions:{" "}
                <AmountDisplay
                  value={totalOldDeductions - STANDARD_DEDUCTION_OLD}
                  currency="INR"
                  isHidden={false}
                />{" "}
                (+ ₹{STANDARD_DEDUCTION_OLD.toLocaleString("en-IN")} std ded)
              </p>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setShowDeductions(!showDeductions)}
              className="h-7 text-xs"
            >
              {showDeductions ? "Hide" : "Edit Deductions"}
            </Button>
          </div>

          {showDeductions && (
            <div className="grid gap-3 pt-2 sm:grid-cols-2 lg:grid-cols-3">
              <div className="space-y-1">
                <Label htmlFor="ded-80c" className="text-muted-foreground text-2xs">
                  Section 80C (EPF, PPF, ELSS, max ₹1.5L)
                </Label>
                <Input
                  id="ded-80c"
                  type="number"
                  min="0"
                  max="150000"
                  value={sec80C}
                  onChange={(e) => setSec80C(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ded-80ccd" className="text-muted-foreground text-2xs">
                  Section 80CCD(1B) (NPS, max ₹50k)
                </Label>
                <Input
                  id="ded-80ccd"
                  type="number"
                  min="0"
                  max="50000"
                  value={sec80CCD}
                  onChange={(e) => setSec80CCD(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ded-80d" className="text-muted-foreground text-2xs">
                  Section 80D (Health Insurance)
                </Label>
                <Input
                  id="ded-80d"
                  type="number"
                  min="0"
                  value={sec80D}
                  onChange={(e) => setSec80D(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ded-hra" className="text-muted-foreground text-2xs">
                  HRA Exemption (House Rent)
                </Label>
                <Input
                  id="ded-hra"
                  type="number"
                  min="0"
                  value={hra}
                  onChange={(e) => setHra(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ded-homeloan" className="text-muted-foreground text-2xs">
                  Sec 24(b) Home Loan Interest (max ₹2L)
                </Label>
                <Input
                  id="ded-homeloan"
                  type="number"
                  min="0"
                  max="200000"
                  value={homeLoanInterest}
                  onChange={(e) => setHomeLoanInterest(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="space-y-1">
                <Label htmlFor="ded-other" className="text-muted-foreground text-2xs">
                  Other Deductions (80E, 80G, etc.)
                </Label>
                <Input
                  id="ded-other"
                  type="number"
                  min="0"
                  value={otherDeductions}
                  onChange={(e) => setOtherDeductions(e.target.value)}
                  className="h-8 text-xs"
                />
              </div>
            </div>
          )}
        </div>

        {/* Side-by-Side Comparison Table */}
        <div className="overflow-x-auto rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[35%]">Component</TableHead>
                <TableHead className="text-end">Old Regime</TableHead>
                <TableHead className="text-end">New Regime</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody className="text-xs">
              <TableRow>
                <TableCell className="font-medium">Gross Total Income</TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={totalGrossIncome} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={totalGrossIncome} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Standard Deduction</TableCell>
                <TableCell className="text-success text-end">
                  − <AmountDisplay value={STANDARD_DEDUCTION_OLD} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-success text-end font-medium">
                  − <AmountDisplay value={STANDARD_DEDUCTION_NEW} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">
                  Chapter VI-A / Other Deductions
                </TableCell>
                <TableCell className="text-success text-end">
                  −{" "}
                  <AmountDisplay
                    value={totalOldDeductions - STANDARD_DEDUCTION_OLD}
                    currency="INR"
                    isHidden={false}
                  />
                </TableCell>
                <TableCell className="text-muted-foreground text-end">Not Allowed</TableCell>
              </TableRow>
              <TableRow className="border-t font-semibold">
                <TableCell>Net Taxable Income</TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={oldTaxableIncome} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={newTaxableIncome} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Tax at Slab Rates</TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={oldTax.slabTax} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={newTax.slabTax} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">
                  Section {sectionNumber("87A", fyStartYear)} Rebate
                </TableCell>
                <TableCell className="text-end">
                  − <AmountDisplay value={oldTax.rebate87a} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  − <AmountDisplay value={newTax.rebate87a} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              {specialIncome > 0 && (
                <>
                  <TableRow>
                    <TableCell className="text-muted-foreground">
                      Tax on Capital Gains at Special Rates
                    </TableCell>
                    <TableCell className="text-end">
                      <AmountDisplay
                        value={oldTax.specialRateTax}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                    <TableCell className="text-end">
                      <AmountDisplay
                        value={newTax.specialRateTax}
                        currency="INR"
                        isHidden={false}
                      />
                    </TableCell>
                  </TableRow>
                </>
              )}
              <TableRow>
                <TableCell className="text-muted-foreground">Surcharge</TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={oldTax.surcharge} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={newTax.surcharge} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              {(oldTax.marginalRelief > 0 || newTax.marginalRelief > 0) && (
                <TableRow>
                  <TableCell className="text-muted-foreground">Marginal Relief</TableCell>
                  <TableCell className="text-end">
                    −{" "}
                    <AmountDisplay value={oldTax.marginalRelief} currency="INR" isHidden={false} />
                  </TableCell>
                  <TableCell className="text-end">
                    −{" "}
                    <AmountDisplay value={newTax.marginalRelief} currency="INR" isHidden={false} />
                  </TableCell>
                </TableRow>
              )}
              <TableRow>
                <TableCell className="text-muted-foreground">
                  Health & Education Cess (4%)
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={oldTax.cess} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-end">
                  <AmountDisplay value={newTax.cess} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow className="bg-muted/30 border-t text-sm font-bold">
                <TableCell>Total Tax Payable</TableCell>
                <TableCell
                  className={`text-end ${recommendedRegime === "old" && !isBreakEven ? "text-success font-bold" : ""}`}
                >
                  <AmountDisplay value={oldTotalTax} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell
                  className={`text-end ${recommendedRegime === "new" && !isBreakEven ? "text-success font-bold" : ""}`}
                >
                  <AmountDisplay value={newTotalTax} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>

        {/* Verdict Callout */}
        <div
          className={`flex items-start gap-3 rounded-lg border p-4 ${
            isBreakEven
              ? "border-info-subtle-border bg-info-subtle text-info-subtle-foreground"
              : recommendedRegime === "new"
                ? "border-success-subtle-border bg-success-subtle text-success-subtle-foreground"
                : "border-warning-subtle-border bg-warning-subtle text-warning-subtle-foreground"
          }`}
        >
          <Icons.CheckCircle className="mt-0.5 h-5 w-5 shrink-0" />
          <div className="space-y-1 text-xs">
            <div className="text-sm font-semibold">
              Recommendation:{" "}
              {isBreakEven
                ? "Choose either regime"
                : recommendedRegime === "new"
                  ? "Opt for New Tax Regime"
                  : "Opt for Old Tax Regime"}
            </div>
            <p className="leading-relaxed">
              {isBreakEven ? (
                "Both regimes result in the exact same tax liability for your current income and deduction parameters."
              ) : recommendedRegime === "new" ? (
                <>
                  The <strong>New Tax Regime</strong> saves you{" "}
                  <strong>₹{difference.toLocaleString("en-IN")}</strong> in tax. Its lower slab
                  rates and ₹{STANDARD_DEDUCTION_NEW.toLocaleString("en-IN")} standard deduction
                  outweigh your Old Regime exemptions.
                </>
              ) : (
                <>
                  The <strong>Old Tax Regime</strong> saves you{" "}
                  <strong>₹{difference.toLocaleString("en-IN")}</strong> in tax due to your claimed
                  deductions of ₹
                  {(totalOldDeductions - STANDARD_DEDUCTION_OLD).toLocaleString("en-IN")} under
                  Section 80C, 80D, and HRA.
                </>
              )}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
