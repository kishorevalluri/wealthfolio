import type { ParsedPdfResult } from "@/lib/types";
import { describe, expect, it } from "vitest";
import { combineContractNotes, NotAllContractNotesError } from "./contract-notes";

const note = (rows: string[][], statementType = "CONTRACT_NOTE"): ParsedPdfResult =>
  ({
    parsed: {
      headers: ["Date", "Side"],
      rows,
      detectedConfig: {},
      errors: [],
      rowCount: rows.length,
    },
    statementType,
    recommendedTemplateId: "system_contract_note",
  }) as unknown as ParsedPdfResult;

describe("combineContractNotes", () => {
  it("puts the rows of several notes together in date order", () => {
    const combined = combineContractNotes([
      { fileName: "b.pdf", result: note([["2024-11-28 12:30:34", "BUY"]]) },
      {
        fileName: "a.pdf",
        result: note([
          ["2020-12-30 10:31:44", "BUY"],
          ["2020-12-30 10:30:01", "SELL"],
        ]),
      },
    ]);
    expect(combined.parsed.rows.map((r) => r[0])).toEqual([
      "2020-12-30 10:30:01",
      "2020-12-30 10:31:44",
      "2024-11-28 12:30:34",
    ]);
    expect(combined.parsed.rowCount).toBe(3);
    expect(combined.recommendedTemplateId).toBe("system_contract_note");
  });

  it("returns a single file as it was read", () => {
    const only = note([["2020-12-30 10:31:44", "BUY"]], "CDSL_CAS");
    expect(combineContractNotes([{ fileName: "cas.pdf", result: only }])).toBe(only);
  });

  it("refuses a mix of contract notes and other statements", () => {
    expect(() =>
      combineContractNotes([
        { fileName: "note.pdf", result: note([]) },
        { fileName: "cas.pdf", result: note([], "CDSL_CAS") },
      ]),
    ).toThrow(NotAllContractNotesError);
  });
});
