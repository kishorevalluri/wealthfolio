import type { IndianTaxYearSummary } from "@/lib/types";
import { Badge } from "@wealthfolio/ui/components/ui/badge";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@wealthfolio/ui/components/ui/card";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { Input } from "@wealthfolio/ui/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@wealthfolio/ui/components/ui/table";
import { AnimatedToggleGroup } from "@wealthfolio/ui";
import React, { useMemo, useRef, useState } from "react";
import { decryptAis, isEncryptedAis } from "../ais/ais-decrypt";
import { parseAisFile, type AisStatement } from "../ais/ais-parser";
import {
  appDividends,
  reconcileAis,
  type DividendRow,
  type ReconStatus,
  type SaleRow,
  type ScripTotals,
} from "../ais/ais-reconcile";

const MANUAL_KEY = "wf.ais.manualPairs";

function loadManual(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(MANUAL_KEY) ?? "{}") as Record<string, string>;
  } catch {
    return {};
  }
}

function saveManual(pairs: Record<string, string>) {
  try {
    localStorage.setItem(MANUAL_KEY, JSON.stringify(pairs));
  } catch {
    // Pairings are a convenience; without storage they last for this visit.
  }
}

function inr(value: number | undefined): string {
  if (value === undefined) return "–";
  return `₹${Math.round(value).toLocaleString("en-IN")}`;
}

function qty(value: number | undefined): string {
  return value === undefined ? "–" : value.toLocaleString("en-IN", { maximumFractionDigits: 3 });
}

function date(value: string | undefined): string {
  if (!value) return "–";
  const [y, m, d] = value.split("-");
  return `${d}/${m}/${y}`;
}

const STATUS: Record<
  ReconStatus,
  { label: string; variant: React.ComponentProps<typeof Badge>["variant"] }
> = {
  match: { label: "Matches", variant: "success-subtle" },
  different: { label: "Differs", variant: "destructive-subtle" },
  "ais-only": { label: "Only in AIS", variant: "warning-subtle" },
  "app-only": { label: "Only in app", variant: "info-subtle" },
};

function StatusBadge({ status }: { status: ReconStatus }) {
  const s = STATUS[status];
  return <Badge variant={s.variant}>{s.label}</Badge>;
}

function Diff({ ais, app }: { ais?: number; app?: number }) {
  if (ais === undefined || app === undefined)
    return <span className="text-muted-foreground">–</span>;
  const d = ais - app;
  if (Math.abs(d) <= Math.max(1, Math.abs(app) * 0.001)) {
    return <span className="text-muted-foreground">0</span>;
  }
  return <span className="text-destructive">{`${d > 0 ? "+" : "−"}${inr(Math.abs(d))}`}</span>;
}

function Hints({ hints }: { hints: string[] }) {
  if (!hints.length) return null;
  return (
    <ul className="text-muted-foreground list-disc space-y-0.5 ps-5 text-xs">
      {hints.map((h) => (
        <li key={h}>{h}</li>
      ))}
    </ul>
  );
}

function Expander({ open }: { open: boolean }) {
  return open ? (
    <Icons.ChevronDown className="h-3.5 w-3.5" />
  ) : (
    <Icons.ChevronRight className="h-3.5 w-3.5" />
  );
}

function DividendTable({
  rows,
  appOptions,
  onPair,
}: {
  rows: DividendRow[];
  appOptions: { id: string; name: string }[];
  onPair: (aisCompany: string, assetId: string | null) => void;
}) {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  if (!rows.length)
    return <p className="text-muted-foreground text-sm">No dividends on either side.</p>;
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Company</TableHead>
          <TableHead className="text-right">AIS gross</TableHead>
          <TableHead className="text-right">App gross</TableHead>
          <TableHead className="text-right">Difference</TableHead>
          <TableHead className="text-right">AIS TDS</TableHead>
          <TableHead className="text-right">App TDS</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => {
          const key = `${row.aisCompany ?? ""}|${row.app?.assetId ?? ""}`;
          const isOpen = !!open[key];
          return (
            <React.Fragment key={key}>
              <TableRow
                className="cursor-pointer"
                onClick={() => setOpen((o) => ({ ...o, [key]: !o[key] }))}
              >
                <TableCell>
                  <span className="flex items-center gap-1.5">
                    <Expander open={isOpen} />
                    {row.name}
                  </span>
                </TableCell>
                <TableCell className="text-right">{inr(row.ais?.gross)}</TableCell>
                <TableCell className="text-right">{inr(row.app?.gross)}</TableCell>
                <TableCell className="text-right">
                  <Diff ais={row.ais?.gross ?? 0} app={row.app?.gross ?? 0} />
                </TableCell>
                <TableCell className="text-right">{inr(row.ais?.tds)}</TableCell>
                <TableCell className="text-right">{inr(row.app?.tds)}</TableCell>
                <TableCell>
                  <StatusBadge status={row.status} />
                </TableCell>
              </TableRow>
              {isOpen && (
                <TableRow className="bg-muted/30 hover:bg-muted/30">
                  <TableCell colSpan={7}>
                    <div className="grid gap-4 py-1 text-xs md:grid-cols-2">
                      <div className="space-y-1">
                        <p className="font-medium">
                          AIS{row.aisCompany ? `: ${row.aisCompany}` : ""}
                        </p>
                        {row.ais ? (
                          row.ais.entries.map((e, i) => (
                            <div key={i} className="flex justify-between gap-3">
                              <span className="text-muted-foreground">
                                {e.code} · {date(e.date)}
                              </span>
                              <span>
                                {inr(e.amount)} {e.tds ? `(TDS ${inr(e.tds)})` : ""}
                              </span>
                            </div>
                          ))
                        ) : (
                          <p className="text-muted-foreground">Nothing in AIS.</p>
                        )}
                      </div>
                      <div className="space-y-1">
                        <p className="font-medium">App</p>
                        {row.app?.payments.length ? (
                          row.app.payments.map((p, i) => (
                            <div key={i} className="flex justify-between gap-3">
                              <span className="text-muted-foreground">{date(p.date)}</span>
                              <span>
                                {inr(p.gross)} {p.tds ? `(TDS ${inr(p.tds)})` : ""}
                              </span>
                            </div>
                          ))
                        ) : (
                          <p className="text-muted-foreground">No dividend recorded.</p>
                        )}
                        {row.aisCompany && row.status !== "match" && (
                          <div
                            className="flex items-center gap-2 pt-2"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <span className="text-muted-foreground">Same company as</span>
                            <Select
                              value={row.app?.assetId ?? ""}
                              onValueChange={(v) => onPair(row.aisCompany!, v || null)}
                            >
                              <SelectTrigger className="h-7 w-56 text-xs">
                                <SelectValue placeholder="Pick a holding" />
                              </SelectTrigger>
                              <SelectContent>
                                {appOptions.map((o) => (
                                  <SelectItem key={o.id} value={o.id}>
                                    {o.name}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                        )}
                      </div>
                    </div>
                    <div className="pt-2">
                      <Hints hints={row.hints} />
                    </div>
                  </TableCell>
                </TableRow>
              )}
            </React.Fragment>
          );
        })}
      </TableBody>
    </Table>
  );
}

function SaleDetail({ row }: { row: SaleRow }) {
  return (
    <div className="space-y-1 border-b py-2 text-xs last:border-b-0">
      <div className="flex flex-wrap items-center gap-2">
        <StatusBadge status={row.status} />
        <span className="font-medium">{date(row.date)}</span>
        {row.aisDate && row.aisDate !== row.date && (
          <span className="text-muted-foreground">(AIS {date(row.aisDate)})</span>
        )}
        {row.app?.intraday && <Badge variant="outline">Intraday</Badge>}
      </div>
      <div className="grid gap-x-6 gap-y-1 md:grid-cols-2">
        <div>
          <p className="text-muted-foreground">AIS</p>
          {row.ais ? (
            <>
              <p>
                {qty(row.ais.quantity)} units · sale value {inr(row.ais.consideration)}
                {row.ais.cost !== undefined && ` · cost ${inr(row.ais.cost)}`}
              </p>
              {row.ais.entries.map((e, i) => (
                <p key={i} className="text-muted-foreground">
                  {e.code}
                  {e.reportedBy ? ` from ${e.reportedBy}` : ""}: {qty(e.quantity)} ×{" "}
                  {e.unitPrice !== undefined ? `₹${e.unitPrice}` : "–"} = {inr(e.consideration)}
                </p>
              ))}
            </>
          ) : (
            <p className="text-muted-foreground">Not in AIS.</p>
          )}
        </div>
        <div>
          <p className="text-muted-foreground">App</p>
          {row.app ? (
            <>
              <p>
                {qty(row.app.quantity)} units · sale value {inr(row.app.consideration)} · cost{" "}
                {inr(row.app.cost)}
                {row.app.deemedCost !== row.app.cost && ` (deemed ${inr(row.app.deemedCost)})`} ·
                gain {inr(row.app.gain)}
              </p>
              {row.app.lots.map((l, i) => (
                <p key={i} className="text-muted-foreground">
                  Bought {date(l.acquisitionDate)}: {qty(l.quantity)} units, cost {inr(l.cost)},
                  gain {inr(l.gain)}
                </p>
              ))}
            </>
          ) : (
            <p className="text-muted-foreground">Not in the app.</p>
          )}
        </div>
      </div>
      <Hints hints={row.hints} />
    </div>
  );
}

function SalesTable({ scrips }: { scrips: ScripTotals[] }) {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  if (!scrips.length)
    return <p className="text-muted-foreground text-sm">No sales on either side.</p>;
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Security</TableHead>
          <TableHead className="text-right">AIS qty</TableHead>
          <TableHead className="text-right">App qty</TableHead>
          <TableHead className="text-right">AIS sale value</TableHead>
          <TableHead className="text-right">App sale value</TableHead>
          <TableHead className="text-right">Difference</TableHead>
          <TableHead className="text-right">App gain</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {scrips.map((s) => {
          const key = `${s.isin ?? ""}|${s.name}`;
          const isOpen = !!open[key];
          return (
            <React.Fragment key={key}>
              <TableRow
                className="cursor-pointer"
                onClick={() => setOpen((o) => ({ ...o, [key]: !o[key] }))}
              >
                <TableCell>
                  <span className="flex items-center gap-1.5">
                    <Expander open={isOpen} />
                    <span>
                      {s.name}
                      {s.isin && (
                        <span className="text-muted-foreground block text-xs">{s.isin}</span>
                      )}
                    </span>
                  </span>
                </TableCell>
                <TableCell className="text-right">{qty(s.aisQuantity)}</TableCell>
                <TableCell className="text-right">{qty(s.appQuantity)}</TableCell>
                <TableCell className="text-right">{inr(s.aisConsideration)}</TableCell>
                <TableCell className="text-right">{inr(s.appConsideration)}</TableCell>
                <TableCell className="text-right">
                  <Diff ais={s.aisConsideration} app={s.appConsideration} />
                </TableCell>
                <TableCell className="text-right">{inr(s.appGain)}</TableCell>
                <TableCell>
                  <StatusBadge status={s.status} />
                </TableCell>
              </TableRow>
              {isOpen && (
                <TableRow className="bg-muted/30 hover:bg-muted/30">
                  <TableCell colSpan={8}>
                    {s.rows.map((r, i) => (
                      <SaleDetail key={i} row={r} />
                    ))}
                  </TableCell>
                </TableRow>
              )}
            </React.Fragment>
          );
        })}
      </TableBody>
    </Table>
  );
}

function Tile({ label, ais, app }: { label: string; ais: number; app: number }) {
  return (
    <div className="rounded-md border p-3 text-sm">
      <p className="text-muted-foreground text-xs">{label}</p>
      <div className="mt-1 flex justify-between">
        <span className="text-muted-foreground">AIS</span>
        <span>{inr(ais)}</span>
      </div>
      <div className="flex justify-between">
        <span className="text-muted-foreground">App</span>
        <span>{inr(app)}</span>
      </div>
      <div className="flex justify-between">
        <span className="text-muted-foreground">Difference</span>
        <Diff ais={ais} app={app} />
      </div>
    </div>
  );
}

/**
 * Loads an AIS file and checks its dividends and sales against this year's
 * figures. The AIS stays in the page; nothing from it is saved or imported.
 */
export function AisReconciliationCard({ summary }: { summary: IndianTaxYearSummary }) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [statement, setStatement] = useState<AisStatement | null>(null);
  const [fileName, setFileName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("differences");
  const [manual, setManual] = useState<Record<string, string>>(loadManual);
  // An encrypted export waits here for the PAN and date of birth.
  const [locked, setLocked] = useState<{ name: string; text: string } | null>(null);
  const [pan, setPan] = useState("");
  const [dob, setDob] = useState("");
  const [opening, setOpening] = useState(false);

  const result = useMemo(
    () => (statement ? reconcileAis(statement, summary, manual) : null),
    [statement, summary, manual],
  );
  const appOptions = useMemo(
    () => appDividends(summary).map((d) => ({ id: d.assetId, name: d.name })),
    [summary],
  );

  const load = (text: string, name: string) => {
    const parsed = parseAisFile(text, name);
    if (!parsed.dividends.length && !parsed.sales.length) {
      throw new Error("No dividends or sales of securities found in this file.");
    }
    setStatement(parsed);
    setFileName(name);
  };

  const onFile = async (file: File | undefined) => {
    if (!file) return;
    setError(null);
    setLocked(null);
    try {
      if (/\.pdf$/i.test(file.name)) {
        throw new Error(
          "PDF is not read yet. On the AIS page, use Download → JSON, or download the CSV of each category.",
        );
      }
      const text = await file.text();
      if (isEncryptedAis(text)) {
        setStatement(null);
        setLocked({ name: file.name, text });
        return;
      }
      load(text, file.name);
    } catch (err) {
      setStatement(null);
      setError(err instanceof Error ? err.message : "Could not read the file.");
    } finally {
      if (inputRef.current) inputRef.current.value = "";
    }
  };

  const onUnlock = async () => {
    if (!locked) return;
    setError(null);
    setOpening(true);
    try {
      load(await decryptAis(locked.text, pan, dob), locked.name);
      setLocked(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not open the file.");
    } finally {
      setOpening(false);
      // The PAN and date of birth are not kept once the file is open.
      setPan("");
      setDob("");
    }
  };

  const onPair = (company: string, assetId: string | null) => {
    const next = { ...manual };
    if (assetId) next[company] = assetId;
    else delete next[company];
    setManual(next);
    saveManual(next);
  };

  const onlyDiff = filter === "differences";
  const dividends = result?.dividends.filter((d) => !onlyDiff || d.status !== "match") ?? [];
  const scrips = result?.scrips.filter((s) => !onlyDiff || s.status !== "match") ?? [];
  const count = (list: { status: ReconStatus }[], status: ReconStatus) =>
    list.filter((x) => x.status === status).length;
  const fyMismatch =
    statement?.fyStartYear !== undefined && statement.fyStartYear !== summary.fyStartYear;

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 pb-2">
        <CardTitle className="text-sm font-medium">Check against AIS</CardTitle>
        <div className="flex items-center gap-2">
          {result && (
            <AnimatedToggleGroup
              variant="secondary"
              size="xs"
              items={[
                { value: "differences", label: "Differences" },
                { value: "all", label: "All" },
              ]}
              value={filter}
              onValueChange={(v) => v && setFilter(v)}
            />
          )}
          <input
            ref={inputRef}
            type="file"
            accept=".json,.csv,.txt,.pdf,application/json,text/csv"
            className="hidden"
            onChange={(e) => void onFile(e.target.files?.[0])}
          />
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5 text-xs"
            onClick={() => inputRef.current?.click()}
          >
            <Icons.Upload className="h-3.5 w-3.5" />
            {statement ? "Load another AIS" : "Load AIS file"}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {!statement && !error && !locked && (
          <p className="text-muted-foreground text-sm">
            Load the AIS from the income-tax portal (Download → JSON, or the CSV of the Dividend and
            Sale of securities categories). Each dividend and sale is checked against the activities
            here, with dates, quantities and amounts on both sides, so you can see whether AIS or
            the app is wrong. The file is read in this window only; nothing from it is saved or
            imported.
          </p>
        )}
        {locked && (
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void onUnlock();
            }}
          >
            <p className="text-muted-foreground w-full text-sm">
              {locked.name} is encrypted. Enter the PAN and date of birth it was downloaded for.
              They open the file here and are not saved.
            </p>
            <Input
              className="h-8 w-36 text-sm uppercase"
              placeholder="PAN"
              autoComplete="off"
              value={pan}
              onChange={(e) => setPan(e.target.value)}
            />
            <Input
              className="h-8 w-36 text-sm"
              placeholder="DD/MM/YYYY"
              autoComplete="off"
              value={dob}
              onChange={(e) => setDob(e.target.value)}
            />
            <Button
              type="submit"
              size="sm"
              className="gap-1.5 text-xs"
              disabled={opening || pan.trim().length !== 10 || !dob.trim()}
            >
              {opening && <Icons.Loader className="h-3.5 w-3.5 animate-spin" />}
              Open
            </Button>
          </form>
        )}
        {error && <p className="text-destructive text-sm">{error}</p>}
        {result && (
          <>
            <p className="text-muted-foreground text-xs">
              {fileName}: {statement!.dividends.length} dividend entries and{" "}
              {statement!.sales.length} sales read. Dividends: {count(result.dividends, "match")}{" "}
              match, {count(result.dividends, "different")} differ,{" "}
              {count(result.dividends, "ais-only")} only in AIS,{" "}
              {count(result.dividends, "app-only")} only in the app. Securities sold:{" "}
              {count(result.scrips, "match")} match, {count(result.scrips, "different")} differ,{" "}
              {count(result.scrips, "ais-only")} only in AIS, {count(result.scrips, "app-only")}{" "}
              only in the app.
            </p>
            {fyMismatch && (
              <p className="text-warning text-sm">
                This AIS is for FY {statement!.fyStartYear}-
                {String((statement!.fyStartYear! + 1) % 100).padStart(2, "0")}, but FY{" "}
                {summary.fyStartYear}-{String((summary.fyStartYear + 1) % 100).padStart(2, "0")} is
                selected. Pick the matching year above.
              </p>
            )}
            <div className="grid gap-3 md:grid-cols-3">
              <Tile
                label="Dividends (gross)"
                ais={result.totals.aisDividends}
                app={result.totals.appDividends}
              />
              <Tile
                label="TDS on dividends"
                ais={result.totals.aisDividendTds}
                app={result.totals.appDividendTds}
              />
              <Tile
                label="Sale value of securities"
                ais={result.totals.aisSaleValue}
                app={result.totals.appSaleValue}
              />
            </div>
            <div className="space-y-2">
              <p className="text-sm font-medium">Dividends by company</p>
              <DividendTable rows={dividends} appOptions={appOptions} onPair={onPair} />
            </div>
            <div className="space-y-2">
              <p className="text-sm font-medium">Sales by security</p>
              <SalesTable scrips={scrips} />
            </div>
            <p className="text-muted-foreground text-xs">
              Amounts within ₹1 count as matching. Click a row for each side's entries. AIS
              dividends reported under both SFT-015 and TDS-194 are counted once. Form 26AS is what
              the department credits TDS from.
            </p>
          </>
        )}
      </CardContent>
    </Card>
  );
}
