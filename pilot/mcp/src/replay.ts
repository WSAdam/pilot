// Replays a recorded task from its plan: each request is filled from the
// fresh responses before it, each response is checked against the recording,
// and the first mismatch stops the run (docs/record-replay.md).
import { describeLocation } from "./analyze.ts";
import type {
  ExtractRule,
  HeaderList,
  PlannedRequest,
  RecordedRequest,
  Recording,
  ReplayPlan,
  Slot,
  ValueSource,
} from "./recording.ts";
import { resultBody } from "./result.ts";
import type { ResultBody } from "./result.ts";
import { baseMimeType, headerValue, htmlTags, isRecord, jsonAt, parseJson } from "./values.ts";
import type { JsonPath } from "./values.ts";

export interface ConcreteRequest {
  id: string;
  method: string;
  url: string;
  headers: HeaderList;
  body?: string;
}

export interface ReplayResponse {
  status: number;
  headers: HeaderList;
  mimeType: string;
  body?: string;
}

/** Sends one request through pilot; the engine never talks to the network itself. */
export type ReplaySend = (request: ConcreteRequest) => Promise<ReplayResponse>;

export interface ReplayOptions {
  inputs: Record<string, string>;
  allowWrites: boolean;
  now?: () => number;
  uuid?: () => string;
}

export type ReplayOutcome =
  | { ok: true; result: ResultBody[]; responses: Record<string, ReplayResponse> }
  | { ok: false; failedStep: number; failedRequest: string; reason: string };

type Resolution = { ok: true; value: string } | { ok: false; reason: string };

export async function replay(
  recording: Recording,
  plan: ReplayPlan,
  send: ReplaySend,
  options: ReplayOptions,
): Promise<ReplayOutcome> {
  const refusal = refuseBeforeSending(plan, options);
  if (refusal !== undefined) {
    return { ok: false, failedStep: 0, failedRequest: "", reason: refusal };
  }
  const recordedById = new Map(recording.requests.map((recorded) => [recorded.id, recorded]));
  const responses: Record<string, ReplayResponse> = {};
  const sentUrls: Record<string, string> = {};
  for (const planned of plan.requests) {
    const recorded = recordedById.get(planned.id);
    if (recorded === undefined) {
      return failure(planned, `the plan names ${planned.id}, which the recording doesn't have`);
    }
    const built = buildRequest(recorded, planned, plan, responses, options);
    if (!built.ok) {
      return failure(planned, built.reason);
    }
    const response = await send(built.request);
    responses[planned.id] = response;
    sentUrls[planned.id] = built.request.url;
    const mismatch = checkResponse(recorded, response);
    if (mismatch !== undefined) {
      return failure(planned, `${recorded.method} ${built.request.url} ${mismatch}`);
    }
  }
  const result = plan.resultRequests.flatMap((id) => {
    const response = responses[id];
    const url = sentUrls[id];
    return response === undefined || url === undefined
      ? []
      : [resultBody(id, url, response.mimeType, response.body ?? "")];
  });
  return { ok: true, result, responses };
}

function failure(planned: PlannedRequest, reason: string): ReplayOutcome {
  return { ok: false, failedStep: planned.step, failedRequest: planned.id, reason };
}

function refuseBeforeSending(plan: ReplayPlan, options: ReplayOptions): string | undefined {
  if (!plan.replayable) {
    return `this recording can't be replayed: ${plan.notReplayableReasons.join("; ")}`;
  }
  const known = Object.keys(plan.inputs);
  const unknown = Object.keys(options.inputs).filter((name) => !known.includes(name));
  if (unknown.length > 0) {
    const takes = known.length > 0 ? known.join(", ") : "no inputs";
    return `unknown input ${unknown.join(", ")}; this task takes: ${takes}`;
  }
  if (plan.writes.length > 0 && !options.allowWrites) {
    const writes = plan.writes.map((write) => `${write.method} ${write.url}`).join(", ");
    return `this task repeats ${plan.writes.length} write(s): ${writes}; pass allow_writes to run it`;
  }
  return undefined;
}

function buildRequest(
  recorded: RecordedRequest,
  planned: PlannedRequest,
  plan: ReplayPlan,
  responses: Record<string, ReplayResponse>,
  options: ReplayOptions,
): { ok: true; request: ConcreteRequest } | { ok: false; reason: string } {
  const url = new URL(recorded.url);
  let headers: HeaderList = recorded.headers.map(([name, value]) => [name, value]);
  let body = recorded.body;
  for (const slot of planned.slots) {
    const resolved = resolve(slot, plan, responses, options);
    if (!resolved.ok) {
      return { ok: false, reason: `${describeLocation(slot.location)}: ${resolved.reason}` };
    }
    if (resolved.value === slot.recorded) {
      continue;
    }
    const location = slot.location;
    switch (location.kind) {
      case "query":
        url.searchParams.set(location.name, resolved.value);
        break;
      case "path":
        url.pathname = replacePathSegment(url.pathname, location.index, resolved.value);
        break;
      case "header":
        headers = headers.map(([name, value]) =>
          name.toLowerCase() === location.name ? [name, resolved.value] : [name, value],
        );
        break;
      case "form": {
        const form = new URLSearchParams(body ?? "");
        form.set(location.name, resolved.value);
        body = form.toString();
        break;
      }
      case "json":
        body = replaceJsonLeaf(body ?? "", location.path, resolved.value);
        break;
    }
  }
  const request: ConcreteRequest = { id: recorded.id, method: recorded.method, url: url.toString(), headers };
  if (body !== undefined) {
    request.body = body;
  }
  return { ok: true, request };
}

function resolve(
  slot: Slot,
  plan: ReplayPlan,
  responses: Record<string, ReplayResponse>,
  options: ReplayOptions,
): Resolution {
  const source: ValueSource = slot.source;
  switch (source.kind) {
    case "constant":
      return { ok: true, value: source.value };
    case "input":
      return { ok: true, value: options.inputs[source.name] ?? plan.inputs[source.name] ?? slot.recorded };
    case "regenerate": {
      const now = (options.now ?? Date.now)();
      if (source.format === "epoch-ms") {
        return { ok: true, value: String(now) };
      }
      if (source.format === "epoch-s") {
        return { ok: true, value: String(Math.floor(now / 1000)) };
      }
      return { ok: true, value: (options.uuid ?? (() => crypto.randomUUID()))() };
    }
    case "extract": {
      const response = responses[source.fromRequest];
      if (response === undefined) {
        return { ok: false, reason: `${source.fromRequest} hasn't been replayed yet` };
      }
      const value = extract(response, source.rule);
      if (value === undefined) {
        return { ok: false, reason: `no ${describeRule(source.rule)} in the response to ${source.fromRequest}` };
      }
      return { ok: true, value };
    }
    case "js-computed":
      return { ok: false, reason: "computed by page JavaScript; it can't be replayed" };
  }
}

function extract(response: ReplayResponse, rule: ExtractRule): string | undefined {
  const body = response.body ?? "";
  switch (rule.in) {
    case "json": {
      const leaf = jsonAt(parseJson(body), rule.path);
      return typeof leaf === "string" || typeof leaf === "number" ? String(leaf) : undefined;
    }
    case "header":
      return headerValue(response.headers, rule.name);
    case "html-input":
      return htmlTags(body, "input").find((attributes) => attributes.get("name") === rule.name)?.get("value");
    case "html-meta":
      return htmlTags(body, "meta")
        .find((attributes) => (attributes.get("name") ?? attributes.get("property")) === rule.name)
        ?.get("content");
    case "text": {
      const start = body.indexOf(rule.before);
      if (start < 0) {
        return undefined;
      }
      const from = start + rule.before.length;
      const end = rule.after.length > 0 ? body.indexOf(rule.after, from) : body.length;
      return end > from ? body.slice(from, end) : undefined;
    }
  }
}

function describeRule(rule: ExtractRule): string {
  switch (rule.in) {
    case "json":
      return `JSON ${rule.path.join(".")}`;
    case "header":
      return `header ${rule.name}`;
    case "html-input":
      return `html-input ${rule.name}`;
    case "html-meta":
      return `html-meta ${rule.name}`;
    case "text":
      return `text between ${JSON.stringify(rule.before)} and ${JSON.stringify(rule.after)}`;
  }
}

function replacePathSegment(pathname: string, index: number, value: string): string {
  let seen = -1;
  return pathname
    .split("/")
    .map((segment) => {
      if (segment.length === 0) {
        return segment;
      }
      seen += 1;
      return seen === index ? encodeURIComponent(value) : segment;
    })
    .join("/");
}

function replaceJsonLeaf(body: string, path: JsonPath, value: string): string {
  const root = parseJson(body);
  const parentPath = path.slice(0, -1);
  const key = path.at(-1);
  const parent = jsonAt(root, parentPath);
  if (key === undefined) {
    return body;
  }
  if (typeof key === "number" && Array.isArray(parent)) {
    parent[key] = typeof parent[key] === "number" ? Number(value) : value;
  } else if (typeof key === "string" && isRecord(parent)) {
    parent[key] = typeof parent[key] === "number" ? Number(value) : value;
  } else {
    return body;
  }
  return JSON.stringify(root);
}

/** Why a fresh response doesn't match the recording, or undefined when it does. */
function checkResponse(recorded: RecordedRequest, response: ReplayResponse): string | undefined {
  const recordedClass = Math.floor(recorded.response.status / 100);
  const freshClass = Math.floor(response.status / 100);
  // fetch() follows redirects, so a recorded 3xx may come back as the 2xx it led to.
  const redirectFollowed = recordedClass === 3 && freshClass === 2;
  if (freshClass !== recordedClass && !redirectFollowed) {
    return `returned ${response.status}; the recording had ${recorded.response.status}`;
  }
  if (recordedClass === 3) {
    return undefined;
  }
  const recordedType = baseMimeType(recorded.response.mimeType);
  const freshType = baseMimeType(response.mimeType);
  if (recordedType.length > 0 && freshType !== recordedType) {
    return `returned ${freshType || "no content type"}; the recording had ${recordedType}`;
  }
  if (recordedType.includes("json")) {
    const recordedBody = parseJson(recorded.response.body ?? "");
    if (isRecord(recordedBody)) {
      const freshBody = parseJson(response.body ?? "");
      const freshKeys = isRecord(freshBody) ? Object.keys(freshBody) : [];
      const missing = Object.keys(recordedBody).filter((key) => !freshKeys.includes(key));
      if (missing.length > 0) {
        return `response lacks ${missing.join(", ")}`;
      }
    }
  }
  return undefined;
}
