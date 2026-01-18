type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null;
}

function readStringField(record: UnknownRecord, key: string): string | null {
  const value = record[key];
  return typeof value === "string" && value.trim().length ? value : null;
}

function extractMessage(value: unknown): { code?: string; message: string } | null {
  if (!isRecord(value)) return null;

  const direct =
    readStringField(value, "message") ??
    readStringField(value, "error") ??
    readStringField(value, "details") ??
    readStringField(value, "reason");

  const code = readStringField(value, "code") ?? undefined;
  if (direct) return { code, message: direct };

  const nestedKeys = ["error", "cause", "data"] as const;
  for (const key of nestedKeys) {
    const nested = extractMessage(value[key]);
    if (nested) {
      return {
        code: code ?? nested.code,
        message: nested.message,
      };
    }
  }

  return null;
}

export function formatError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message || String(error);

  const extracted = extractMessage(error);
  if (extracted) {
    return extracted.code ? `${extracted.code}: ${extracted.message}` : extracted.message;
  }

  try {
    const text = String(error);
    if (text && text !== "[object Object]") return text;
  } catch {
    // ignore
  }

  try {
    return JSON.stringify(error);
  } catch {
    return "Unknown error";
  }
}

