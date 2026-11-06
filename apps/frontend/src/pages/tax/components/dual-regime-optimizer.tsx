import { AmountDisplay } from "@wealthfolio/ui";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@wealthfolio/ui/components/ui/card";
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

// ── Slabs & Constants ────────────────────────────────────────────────────────

const STANDARD_DEDUCTION_NEW = 75_000;
const STANDARD_DEDUCTION_OLD = 50_000;

// Old Regime (individuals under 60)
// Up to 2.5L: 0%
// 2.5L - 5L: 5%
// 5L - 10L: 20%
// > 10L: 30%
function calculateOldRegimeTax(taxableIncome: number): number {
  if (taxableIncome <= 0) return 0;
  let tax = 0;
  if (taxableIncome > 1_000_000) {
    tax += (taxableIncome - 1_000_000) * 0.30;
    tax += 500_000 * 0.20; // 5L to 10L
    tax += 250_000 * 0.05; // 2.5L to 5L
  } else if (taxableIncome > 500_000) {
    tax += (taxableIncome - 500_000) * 0.20;
    tax += 250_000 * 0.05;
  } else if (taxableIncome > 250_000) {
    tax += (taxableIncome - 250_000) * 0.05;
  }

  // Section 87A rebate for Old Regime: if taxable income <= 5,00,000, max rebate ₹12,500
  if (taxableIncome <= 500_000) {
    tax = Math.max(0, tax - 12_500);
  }

  return tax;
}

// New Regime (Budget 2024 / FY 2024-25 onwards)
// Up to 3L: 0%
// 3L - 7L: 5%
// 7L - 10L: 10%
// 10L - 12L: 15%
// 12L - 15L: 20%
// > 15L: 30%
function calculateNewRegimeTax(taxableIncome: number): number {
  if (taxableIncome <= 0) return 0;
  let tax = 0;
  if (taxableIncome > 1_500_000) {
    tax += (taxableIncome - 1_500_000) * 0.30;
    tax += 300_000 * 0.20; // 12L to 15L
    tax += 200_000 * 0.15; // 10L to 12L
    tax += 300_000 * 0.10; // 7L to 10L
    tax += 400_000 * 0.05; // 3L to 7L
  } else if (taxableIncome > 1_200_000) {
    tax += (taxableIncome - 1_200_000) * 0.20;
    tax += 200_000 * 0.15;
    tax += 300_000 * 0.10;
    tax += 400_000 * 0.05;
  } else if (taxableIncome > 1_000_000) {
    tax += (taxableIncome - 1_000_000) * 0.15;
    tax += 300_000 * 0.10;
    tax += 400_000 * 0.05;
  } else if (taxableIncome > 700_000) {
    tax += (taxableIncome - 700_000) * 0.10;
    tax += 400_000 * 0.05;
  } else if (taxableIncome > 300_000) {
    tax += (taxableIncome - 300_000) * 0.05;
  }

  // Section 87A rebate for New Regime:
  // Full rebate if taxable income <= 7,00,000 (tax liability becomes 0)
  if (taxableIncome <= 700_000) {
    tax = 0;
  } else {
    // Marginal relief under Section 87A for income slightly exceeding 7,00,000:
    // Tax payable cannot exceed (Taxable Income - 7,00,000)
    const excessOver7L = taxableIncome - 700_000;
    if (tax > excessOver7L) {
      tax = excessOver7L;
    }
  }

  return tax;
}

export interface DualRegimeOptimizerProps {
  /** Capital gains from portfolio to include in calculation */
  stcgSlabTotal?: number;
}

export function DualRegimeOptimizer({ stcgSlabTotal = 0 }: DualRegimeOptimizerProps) {
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

  // Parsed values
  const numSalary = Math.max(0, parseFloat(grossSalary) || 0);
  const numOther = Math.max(0, parseFloat(otherIncome) || 0);
  const additionalSlabGains = includePortfolioSlabGains ? Math.max(0, stcgSlabTotal) : 0;
  const totalGrossIncome = numSalary + numOther + additionalSlabGains;

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

  // Taxable income
  const oldTaxableIncome = Math.max(0, totalGrossIncome - totalOldDeductions);
  const newTaxableIncome = Math.max(0, totalGrossIncome - STANDARD_DEDUCTION_NEW);

  // Tax before cess
  const oldTaxBeforeCess = calculateOldRegimeTax(oldTaxableIncome);
  const newTaxBeforeCess = calculateNewRegimeTax(newTaxableIncome);

  // 4% Health & Education Cess
  const oldCess = Math.round(oldTaxBeforeCess * 0.04);
  const newCess = Math.round(newTaxBeforeCess * 0.04);

  const oldTotalTax = oldTaxBeforeCess + oldCess;
  const newTotalTax = newTaxBeforeCess + newCess;

  const difference = Math.abs(oldTotalTax - newTotalTax);
  const recommendedRegime = newTotalTax <= oldTotalTax ? "new" : "old";
  const isBreakEven = oldTotalTax === newTotalTax;

  return (
    <Card className="border-border">
      <CardHeader>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2 text-base font-semibold">
              <Icons.Receipt className="h-5 w-5 text-primary" />
              Dual-Regime Tax Optimizer (Budget 2024)
            </CardTitle>
            <CardDescription className="text-xs">
              Interactive comparison of Old vs. New Tax Regime with ₹75,000 standard deduction,
              revised slabs, and Section 87A rebate.
            </CardDescription>
          </div>
          <Badge
            variant={recommendedRegime === "new" ? "default" : "secondary"}
            className="w-fit text-xs font-semibold px-3 py-1"
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

          {stcgSlabTotal > 0 && (
            <div className="flex flex-col justify-end space-y-1.5">
              <div className="flex items-center justify-between rounded-md border p-2">
                <div className="space-y-0.5">
                  <span className="text-xs font-medium">Include Slab Gains</span>
                  <p className="text-muted-foreground text-[10px]">
                    +₹{Math.round(stcgSlabTotal).toLocaleString("en-IN")} from portfolio
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
        <div className="space-y-3 rounded-lg border bg-muted/20 p-3">
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <div className="text-xs font-medium">Old Regime Deductions (Chapter VI-A & HRA)</div>
              <p className="text-muted-foreground text-[11px]">
                Active deductions: <AmountDisplay value={totalOldDeductions - STANDARD_DEDUCTION_OLD} currency="INR" isHidden={false} /> (+ ₹50,000 std ded)
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
                <Label htmlFor="ded-80c" className="text-[11px] text-muted-foreground">
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
                <Label htmlFor="ded-80ccd" className="text-[11px] text-muted-foreground">
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
                <Label htmlFor="ded-80d" className="text-[11px] text-muted-foreground">
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
                <Label htmlFor="ded-hra" className="text-[11px] text-muted-foreground">
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
                <Label htmlFor="ded-homeloan" className="text-[11px] text-muted-foreground">
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
                <Label htmlFor="ded-other" className="text-[11px] text-muted-foreground">
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
                <TableHead className="text-right">Old Regime</TableHead>
                <TableHead className="text-right">New Regime (Budget 2024)</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody className="text-xs">
              <TableRow>
                <TableCell className="font-medium">Gross Total Income</TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={totalGrossIncome} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={totalGrossIncome} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Standard Deduction</TableCell>
                <TableCell className="text-right text-success">
                  − <AmountDisplay value={STANDARD_DEDUCTION_OLD} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right text-success font-medium">
                  − <AmountDisplay value={STANDARD_DEDUCTION_NEW} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Chapter VI-A / Other Deductions</TableCell>
                <TableCell className="text-right text-success">
                  − <AmountDisplay value={totalOldDeductions - STANDARD_DEDUCTION_OLD} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right text-muted-foreground">
                  Not Allowed
                </TableCell>
              </TableRow>
              <TableRow className="border-t font-semibold">
                <TableCell>Net Taxable Income</TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={oldTaxableIncome} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={newTaxableIncome} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Income Tax (after Sec 87A rebate)</TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={oldTaxBeforeCess} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={newTaxBeforeCess} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell className="text-muted-foreground">Health & Education Cess (4%)</TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={oldCess} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className="text-right">
                  <AmountDisplay value={newCess} currency="INR" isHidden={false} />
                </TableCell>
              </TableRow>
              <TableRow className="border-t bg-muted/30 text-sm font-bold">
                <TableCell>Total Tax Payable</TableCell>
                <TableCell className={`text-right ${recommendedRegime === "old" && !isBreakEven ? "text-success font-bold" : ""}`}>
                  <AmountDisplay value={oldTotalTax} currency="INR" isHidden={false} />
                </TableCell>
                <TableCell className={`text-right ${recommendedRegime === "new" && !isBreakEven ? "text-success font-bold" : ""}`}>
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
              ? "border-blue-500/20 bg-blue-500/5 text-blue-900 dark:text-blue-200"
              : recommendedRegime === "new"
                ? "border-emerald-500/20 bg-emerald-500/5 text-emerald-950 dark:text-emerald-200"
                : "border-amber-500/20 bg-amber-500/5 text-amber-950 dark:text-amber-200"
          }`}
        >
          <Icons.CheckCircle className="mt-0.5 h-5 w-5 shrink-0" />
          <div className="space-y-1 text-xs">
            <div className="font-semibold text-sm">
              Recommendation: {isBreakEven ? "Choose either regime" : recommendedRegime === "new" ? "Opt for New Tax Regime" : "Opt for Old Tax Regime"}
            </div>
            <p className="leading-relaxed">
              {isBreakEven ? (
                "Both regimes result in the exact same tax liability for your current income and deduction parameters."
              ) : recommendedRegime === "new" ? (
                <>
                  The <strong>New Tax Regime</strong> saves you{" "}
                  <strong>₹{difference.toLocaleString("en-IN")}</strong> in tax. Its lower slab rates and enhanced ₹75,000 standard deduction outweigh your Old Regime exemptions.
                </>
              ) : (
                <>
                  The <strong>Old Tax Regime</strong> saves you{" "}
                  <strong>₹{difference.toLocaleString("en-IN")}</strong> in tax due to your claimed deductions of{" "}
                  ₹{(totalOldDeductions - STANDARD_DEDUCTION_OLD).toLocaleString("en-IN")} under Section 80C, 80D, and HRA.
                </>
              )}
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
