import type { ExportDataType, ExportedFileFormat } from "@/lib/types";
import { notifyUnauthorized } from "@/lib/auth-token";
import type { DataExportResult } from "../types";
import { API_PREFIX } from "./core";

const fallbackFileName = (data: ExportDataType, format: ExportedFileFormat): string => {
  const currentDate = new Date().toISOString().split("T")[0];
  return `${data}_${currentDate}.${format.toLowerCase()}`;
};

const filenameFromContentDisposition = (value: string | null): string | null => {
  if (!value) return null;

  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(value);
  if (utf8Match?.[1]) {
    return decodeURIComponent(utf8Match[1]);
  }

  const quotedMatch = /filename="([^"]+)"/i.exec(value);
  if (quotedMatch?.[1]) {
    return quotedMatch[1];
  }

  const bareMatch = /filename=([^;]+)/i.exec(value);
  return bareMatch?.[1]?.trim() ?? null;
};

const downloadBlob = (blob: Blob, fileName: string) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
};

export const exportDataFile = async (
  format: ExportedFileFormat,
  data: ExportDataType,
): Promise<DataExportResult> => {
  const url = `${API_PREFIX}/utilities/export/${encodeURIComponent(data)}/${encodeURIComponent(
    format.toLowerCase(),
  )}`;

  const response = await fetch(url, {
    method: "GET",
    credentials: "same-origin",
  });

  if (response.status === 401) {
    notifyUnauthorized();
  }

  if (response.status === 204) {
    return { status: "empty" };
  }

  if (!response.ok) {
    let message = response.statusText;
    try {
      const error = await response.json();
      message = (error?.message ?? message) as string;
    } catch {
      // Keep the HTTP status text when the server did not return JSON.
    }
    throw new Error(message);
  }

  const filename =
    filenameFromContentDisposition(response.headers.get("Content-Disposition")) ??
    fallbackFileName(data, format);
  const blob = await response.blob();
  downloadBlob(blob, filename);

  return { status: "saved", filename };
};

export const exportIndianScheduleCg = async (
  filter?: unknown,
  financialYear?: number,
): Promise<DataExportResult> => {
  const url = `${API_PREFIX}/tax/indian/schedule-cg/export/query`;

  const response = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    credentials: "same-origin",
    body: JSON.stringify({
      filter: filter ?? null,
      financialYear: financialYear ?? null,
    }),
  });

  if (response.status === 401) {
    notifyUnauthorized();
  }

  if (response.status === 204) {
    return { status: "empty" };
  }

  if (!response.ok) {
    let message = response.statusText;
    try {
      const error = await response.json();
      message = (error?.message ?? message) as string;
    } catch {
      // Keep HTTP status text
    }
    throw new Error(message);
  }

  const defaultFilename = `schedule_cg_${financialYear ?? "current"}.csv`;
  const filename =
    filenameFromContentDisposition(response.headers.get("Content-Disposition")) ?? defaultFilename;
  const blob = await response.blob();
  downloadBlob(blob, filename);

  return { status: "saved", filename };
};
