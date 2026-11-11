// Small parsing helpers shared by the analyzer and the replay engine.
import type { HeaderList } from "./recording.ts";

export type JsonPath = (string | number)[];

export function parseJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Every string and number leaf, with its path, in document order. */
export function jsonLeaves(value: unknown, path: JsonPath = []): { path: JsonPath; value: string }[] {
  if (typeof value === "string" || typeof value === "number") {
    return [{ path, value: String(value) }];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item, index) => jsonLeaves(item, [...path, index]));
  }
  if (isRecord(value)) {
    return Object.entries(value).flatMap(([key, item]) => jsonLeaves(item, [...path, key]));
  }
  return [];
}

export function jsonAt(value: unknown, path: JsonPath): unknown {
  let current = value;
  for (const key of path) {
    if (typeof key === "number" && Array.isArray(current)) {
      current = current[key];
    } else if (typeof key === "string" && isRecord(current)) {
      current = current[key];
    } else {
      return undefined;
    }
  }
  return current;
}

export function headerValue(headers: HeaderList, wanted: string): string | undefined {
  const name = wanted.toLowerCase();
  return headers.find(([candidate]) => candidate.toLowerCase() === name)?.[1];
}

export function htmlTags(html: string, tagName: "input" | "meta"): Map<string, string>[] {
  const pattern = tagName === "input" ? /<input\b[^>]*>/gi : /<meta\b[^>]*>/gi;
  return (html.match(pattern) ?? []).map(tagAttributes);
}

function tagAttributes(tag: string): Map<string, string> {
  const attributes = new Map<string, string>();
  const pattern = /([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))/g;
  for (const match of tag.matchAll(pattern)) {
    const name = match[1];
    if (name !== undefined) {
      attributes.set(name.toLowerCase(), decodeHtmlEntities(match[2] ?? match[3] ?? match[4] ?? ""));
    }
  }
  return attributes;
}

function decodeHtmlEntities(text: string): string {
  return text
    .replaceAll("&quot;", '"')
    .replaceAll("&#39;", "'")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&");
}

/** The media type without parameters: "text/html; charset=utf-8" → "text/html". */
export function baseMimeType(mimeType: string): string {
  return (mimeType.split(";")[0] ?? "").trim().toLowerCase();
}
