import type { ParsedPdfResult } from "@/lib/types";

/** Thrown when several files are picked and one of them is not a contract note. */
export class NotAllContractNotesError extends Error {
  constructor(public readonly fileName: string) {
    super(`${fileName} is not a broker contract note`);
    this.name = "NotAllContractNotesError";
  }
}

/**
 * Several contract notes read as one import: their rows together, in date
 * order (the parser writes the date and time first), under the first note's
 * headers and template. Each note's charges stay on its own rows.
 */
export function combineContractNotes(
  results: { fileName: string; result: ParsedPdfResult }[],
): ParsedPdfResult {
  if (results.length === 0) {
    throw new Error("No files to combine");
  }
  if (results.length === 1) {
    return results[0].result;
  }
  const other = results.find(({ result }) => result.statementType !== "CONTRACT_NOTE");
  if (other) {
    throw new NotAllContractNotesError(other.fileName);
  }
  const first = results[0].result;
  const rows = results
    .flatMap(({ result }) => result.parsed.rows)
    .map((row, index) => ({ row, index }))
    .sort((a, b) => (a.row[0] ?? "").localeCompare(b.row[0] ?? "") || a.index - b.index)
    .map(({ row }) => row);
  return {
    ...first,
    parsed: {
      ...first.parsed,
      rows,
      rowCount: rows.length,
      errors: results.flatMap(({ result }) => result.parsed.errors ?? []),
    },
  };
}
