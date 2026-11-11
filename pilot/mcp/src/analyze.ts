// Turns a recorded task into a replay plan: every value in every kept request
// is traced to the place a later run gets it from (docs/record-replay.md,
// "Values that change between runs").
import type {
  ExtractRule,
  PlannedRequest,
  PlannedWrite,
  RecordedRequest,
  Recording,
  ReplayPlan,
  Slot,
  SlotLocation,
  ValueSource,
} from "./recording.ts";
import { findResultRequests } from "./result.ts";
import { headerValue, htmlTags, isRecord, jsonLeaves, parseJson } from "./values.ts";

/** Request headers the browser or the cookie jar owns; they're never slots. */
const BROWSER_OWNED_HEADERS = new Set([
  "accept",
  "accept-encoding",
  "accept-language",
  "cache-control",
  "connection",
  "content-length",
  "content-type",
  "cookie",
  "dnt",
  "host",
  "origin",
  "pragma",
  "priority",
  "referer",
  "upgrade-insecure-requests",
  "user-agent",
]);

const READ_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Free-text anchors on each side of an extracted value. */
const TEXT_ANCHOR_LENGTH = 16;

interface NamedInput {
  name: string;
  value: string;
  /** The step that typed it; task_start inputs are available from step 0. */
  step: number;
}

export function analyzeRecording(recording: Recording): ReplayPlan {
  const inputs = collectInputs(recording);
  const usedInputs: Record<string, string> = {};
  const notReplayableReasons: string[] = [];
  const requests: PlannedRequest[] = recording.requests.map((request, index) => {
    const earlier = recording.requests.slice(0, index).reverse();
    const slots: Slot[] = slotValues(request).map(({ location, value }) => {
      const source = sourceFor(value, request, earlier, inputs);
      if (source.kind === "input") {
        usedInputs[source.name] = value;
      }
      if (source.kind === "js-computed") {
        notReplayableReasons.push(
          `${request.id} ${request.method} ${request.url}: ${describeLocation(location)} looks computed by page JavaScript`,
        );
      }
      return { location, recorded: value, source };
    });
    return { id: request.id, step: request.step, method: request.method, url: request.url, slots };
  });
  const writes: PlannedWrite[] = recording.requests
    .filter((request) => isWrite(request))
    .map(({ id, step, method, url }) => ({ id, step, method, url }));
  return {
    name: recording.name,
    inputs: usedInputs,
    requests,
    writes,
    resultRequests: findResultRequests(recording),
    replayable: notReplayableReasons.length === 0,
    notReplayableReasons,
  };
}

function isWrite(request: RecordedRequest): boolean {
  return !READ_METHODS.has(request.method.toUpperCase());
}

function collectInputs(recording: Recording): NamedInput[] {
  const named: NamedInput[] = Object.entries(recording.inputs).map(([name, value]) => ({
    name,
    value,
    step: 0,
  }));
  for (const step of recording.steps) {
    const { args } = step;
    if (step.tool === "browser_type" && typeof args.text === "string") {
      named.push({ name: `step${step.index}_text`, value: args.text, step: step.index });
    }
    if (step.tool === "browser_fill_form" && Array.isArray(args.fields)) {
      for (const field of args.fields) {
        if (isRecord(field) && typeof field.name === "string" && typeof field.value === "string") {
          named.push({
            name: `step${step.index}_${slug(field.name)}`,
            value: field.value,
            step: step.index,
          });
        }
      }
    }
    if (step.tool === "browser_select_option" && Array.isArray(args.values)) {
      args.values.forEach((value, position) => {
        if (typeof value === "string") {
          named.push({ name: `step${step.index}_option${position}`, value, step: step.index });
        }
      });
    }
  }
  return named;
}

function slotValues(request: RecordedRequest): { location: SlotLocation; value: string }[] {
  const slots: { location: SlotLocation; value: string }[] = [];
  const url = new URL(request.url);
  url.pathname
    .split("/")
    .filter((segment) => segment.length > 0)
    .forEach((segment, index) => {
      const value = decodeURIComponent(segment);
      // Only segments that carry a digit can be ids; words like "orders" are routes.
      if (/\d/.test(value)) {
        slots.push({ location: { kind: "path", index }, value });
      }
    });
  for (const [name, value] of url.searchParams) {
    slots.push({ location: { kind: "query", name }, value });
  }
  for (const [rawName, value] of request.headers) {
    const name = rawName.toLowerCase();
    if (!BROWSER_OWNED_HEADERS.has(name) && !name.startsWith("sec-")) {
      slots.push({ location: { kind: "header", name }, value });
    }
  }
  if (request.body !== undefined && request.body.length > 0) {
    const contentType = headerValue(request.headers, "content-type") ?? "";
    if (contentType.includes("application/x-www-form-urlencoded")) {
      for (const [name, value] of new URLSearchParams(request.body)) {
        slots.push({ location: { kind: "form", name }, value });
      }
    } else if (contentType.includes("json")) {
      const parsed = parseJson(request.body);
      if (parsed !== undefined) {
        for (const leaf of jsonLeaves(parsed)) {
          slots.push({ location: { kind: "json", path: leaf.path }, value: leaf.value });
        }
      }
    }
  }
  return slots;
}

function sourceFor(
  value: string,
  request: RecordedRequest,
  earlierNewestFirst: RecordedRequest[],
  inputs: NamedInput[],
): ValueSource {
  const input = inputs.find((candidate) => candidate.step <= request.step && candidate.value === value);
  if (input !== undefined) {
    return { kind: "input", name: input.name };
  }
  if (isDistinctive(value)) {
    for (const earlier of earlierNewestFirst) {
      const rule = structuredExtractRule(earlier, value);
      if (rule !== undefined) {
        return { kind: "extract", fromRequest: earlier.id, rule };
      }
    }
  }
  const regenerated = regenerateFormat(value);
  if (regenerated !== undefined) {
    return { kind: "regenerate", format: regenerated };
  }
  if (isDistinctive(value)) {
    for (const earlier of earlierNewestFirst) {
      const rule = textExtractRule(earlier.response.body, value);
      if (rule !== undefined) {
        return { kind: "extract", fromRequest: earlier.id, rule };
      }
    }
  }
  if (isWrite(request) && looksComputed(value)) {
    return { kind: "js-computed", value };
  }
  return { kind: "constant", value };
}

/** A value worth tracing: short or digit-free values ("true", "compact") stay constants. */
function isDistinctive(value: string): boolean {
  return value.length >= 4 && (/\d/.test(value) || value.length >= 12);
}

function looksComputed(value: string): boolean {
  if (/^[0-9a-f]{16,}$/i.test(value)) {
    return true;
  }
  return /^[A-Za-z0-9+/_-]{20,}={0,2}$/.test(value) && /\d/.test(value) && /[A-Za-z]/.test(value);
}

function regenerateFormat(value: string): "epoch-ms" | "epoch-s" | "uuid" | undefined {
  if (/^\d{13}$/.test(value)) {
    const millis = Number(value);
    if (millis >= 1e12 && millis <= 4.1e12) {
      return "epoch-ms";
    }
  }
  if (/^\d{10}$/.test(value)) {
    const seconds = Number(value);
    if (seconds >= 1e9 && seconds <= 4.1e9) {
      return "epoch-s";
    }
  }
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)) {
    return "uuid";
  }
  return undefined;
}

function structuredExtractRule(earlier: RecordedRequest, value: string): ExtractRule | undefined {
  const body = earlier.response.body;
  if (body !== undefined && earlier.response.mimeType.includes("json")) {
    const parsed = parseJson(body);
    if (parsed !== undefined) {
      const leaf = jsonLeaves(parsed).find((candidate) => candidate.value === value);
      if (leaf !== undefined) {
        return { in: "json", path: leaf.path };
      }
    }
  }
  for (const [rawName, headerText] of earlier.response.headers) {
    const name = rawName.toLowerCase();
    // Cookies come from the jar at replay, never from a recorded Set-Cookie.
    if (name !== "set-cookie" && headerText === value) {
      return { in: "header", name };
    }
  }
  if (body !== undefined && earlier.response.mimeType.includes("html")) {
    for (const attributes of htmlTags(body, "input")) {
      const name = attributes.get("name");
      if (attributes.get("value") === value && name !== undefined) {
        return { in: "html-input", name };
      }
    }
    for (const attributes of htmlTags(body, "meta")) {
      const name = attributes.get("name") ?? attributes.get("property");
      if (attributes.get("content") === value && name !== undefined) {
        return { in: "html-meta", name };
      }
    }
  }
  return undefined;
}

function textExtractRule(body: string | undefined, value: string): ExtractRule | undefined {
  if (body === undefined) {
    return undefined;
  }
  const at = body.indexOf(value);
  if (at < 0) {
    return undefined;
  }
  return {
    in: "text",
    before: body.slice(Math.max(0, at - TEXT_ANCHOR_LENGTH), at),
    after: body.slice(at + value.length, at + value.length + TEXT_ANCHOR_LENGTH),
  };
}

export function describeLocation(location: SlotLocation): string {
  switch (location.kind) {
    case "query":
      return `query ${location.name}`;
    case "path":
      return `path segment ${location.index}`;
    case "form":
      return `form field ${location.name}`;
    case "json":
      return `json ${location.path.join(".")}`;
    case "header":
      return `header ${location.name}`;
  }
}

function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}
