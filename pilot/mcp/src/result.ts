// Which recorded responses hold the answer the agent read, and how a fresh
// one is handed back (docs/record-replay.md, "Knowing which response is the
// answer").
import type { RecordedRequest, Recording } from "./recording.ts";
import { baseMimeType, decodeHtmlEntities, jsonLeaves, parseJson } from "./values.ts";

/** Snapshot lines under these roles are page furniture, not the answer. */
const FURNITURE_ROLES = new Set(["banner", "contentinfo", "navigation"]);
/** A response joins the result only if it explains this many more snapshot terms. */
const MIN_NEW_TERMS = 2;
const MAX_RESULT_RESPONSES = 5;
const MAX_RESULT_TEXT = 20_000;

export interface ResultBody {
  id: string;
  url: string;
  contentType: string;
  text: string;
}

export function findResultRequests(recording: Recording): string[] {
  const withBody = recording.requests.filter((request) => (request.response.body ?? "").length > 0);
  const fallback = withBody.at(-1);
  const snapshotTerms = recording.finalSnapshot === undefined ? new Set<string>() : snapshotTermsOf(recording.finalSnapshot);
  const candidates = withBody.map((request, order) => ({
    request,
    order,
    terms: intersect(termsOf(responseText(request.response.mimeType, request.response.body ?? "")), snapshotTerms),
  }));
  const covered = new Set<string>();
  const chosen: { request: RecordedRequest; order: number }[] = [];
  while (chosen.length < MAX_RESULT_RESPONSES) {
    let best: (typeof candidates)[number] | undefined;
    let bestGain = 0;
    for (const candidate of candidates) {
      if (chosen.some((entry) => entry.request.id === candidate.request.id)) {
        continue;
      }
      const gain = [...candidate.terms].filter((term) => !covered.has(term)).length;
      // Ties go to the later response: it carries the newest state.
      if (gain >= bestGain && gain > 0) {
        best = candidate;
        bestGain = gain;
      }
    }
    if (best === undefined || bestGain < MIN_NEW_TERMS) {
      break;
    }
    chosen.push(best);
    for (const term of best.terms) {
      covered.add(term);
    }
  }
  if (chosen.length === 0) {
    return fallback === undefined ? [] : [fallback.id];
  }
  return chosen.sort((left, right) => left.order - right.order).map((entry) => entry.request.id);
}

export function resultBody(id: string, url: string, mimeType: string, body: string): ResultBody {
  const contentType = baseMimeType(mimeType);
  const text = contentType.includes("html") ? htmlToText(body) : body;
  return { id, url, contentType, text: text.slice(0, MAX_RESULT_TEXT) };
}

function snapshotTermsOf(snapshot: string): Set<string> {
  const terms = new Set<string>();
  let furnitureIndent: number | undefined;
  for (const line of snapshot.split("\n")) {
    const indent = line.length - line.trimStart().length;
    if (furnitureIndent !== undefined && indent > furnitureIndent) {
      continue;
    }
    furnitureIndent = undefined;
    const role = /^\s*-\s*([a-z]+)/.exec(line)?.[1];
    if (role !== undefined && FURNITURE_ROLES.has(role)) {
      furnitureIndent = indent;
      continue;
    }
    const text = line
      .replace(/\[[a-z]+(=[^\]]*)?\]/g, " ")
      .replace(/^\s*-\s*[a-z]+/, " ");
    for (const term of termsOf(text)) {
      terms.add(term);
    }
  }
  return terms;
}

function responseText(mimeType: string, body: string): string {
  const type = baseMimeType(mimeType);
  if (type.includes("json")) {
    const parsed = parseJson(body);
    return parsed === undefined ? body : jsonLeaves(parsed).map((leaf) => leaf.value).join(" ");
  }
  if (type.includes("html")) {
    return htmlToText(body);
  }
  return body;
}

function termsOf(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .split(/[\s,;:()[\]{}"'<>|]+/)
      .map((term) => term.replace(/\.+$/, ""))
      .filter((term) => term.length >= 3),
  );
}

function intersect(left: Set<string>, right: Set<string>): Set<string> {
  return new Set([...left].filter((term) => right.has(term)));
}

function htmlToText(html: string): string {
  return decodeHtmlEntities(
    html
      .replace(/<(script|style|noscript|template)\b[\s\S]*?<\/\1>/gi, " ")
      .replace(/<[^>]+>/g, " "),
  )
    .replace(/\s+/g, " ")
    .trim();
}
