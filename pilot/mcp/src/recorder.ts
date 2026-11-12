// Records the requests a task fires, step by step, from the BrowserContext
// pilot-mcp owns and hands to Playwright MCP (docs/record-replay.md, spike
// results). Bodies are read as each request finishes: pilot forgets a
// document's body once the next document loads. Logs go to stderr: stdout
// carries the MCP stdio protocol.
import type { BrowserContext, Request } from "playwright-core";

import { keepRequest, recordableHeaders } from "./filter.ts";
import type { HeaderList, RecordedRequest, RecordedStep, Recording } from "./recording.ts";

interface Seen {
  order: number;
  step: number;
  request: Request;
  body: Promise<string | undefined> | undefined;
}

export class TaskRecorder {
  #name = "";
  #inputs: Record<string, string> = {};
  #steps: RecordedStep[] = [];
  #seen = new Map<Request, Seen>();
  #order = 0;
  #active = false;

  constructor(context: BrowserContext) {
    context.on("request", (request) => {
      if (this.#active) {
        this.#seen.set(request, { order: this.#order++, step: this.#steps.length, request, body: undefined });
      }
    });
    context.on("requestfinished", (request) => {
      const seen = this.#seen.get(request);
      if (seen !== undefined) {
        seen.body = request
          .response()
          .then((response) => response?.text())
          .catch(() => undefined);
      }
    });
  }

  get active(): boolean {
    return this.#active;
  }

  start(name: string, inputs: Record<string, string>): void {
    this.#name = name;
    this.#inputs = { ...inputs };
    this.#steps = [];
    this.#seen = new Map();
    this.#order = 0;
    this.#active = true;
    console.error(`🚀 pilot-mcp recording task "${name}"`);
  }

  /** Marks the start of one forwarded tool call; requests after it belong to it. */
  beginStep(tool: string, args: Record<string, unknown>): void {
    if (this.#active) {
      this.#steps.push({ index: this.#steps.length + 1, tool, args });
    }
  }

  async finish(finalSnapshot?: string): Promise<Recording> {
    this.#active = false;
    const seen = [...this.#seen.values()].sort((left, right) => left.order - right.order);
    const ids = new Map(seen.map((entry) => [entry.request, `r${entry.order}`]));
    const requests: RecordedRequest[] = [];
    for (const entry of seen) {
      const record = await toRecordedRequest(entry, ids);
      if (record !== undefined) {
        requests.push(record);
      }
    }
    console.error(`✅ pilot-mcp recorded "${this.#name}": ${this.#steps.length} steps, ${requests.length} requests kept`);
    const recording: Recording = { name: this.#name, inputs: this.#inputs, steps: this.#steps, requests };
    if (finalSnapshot !== undefined) {
      recording.finalSnapshot = finalSnapshot;
    }
    return recording;
  }
}

async function toRecordedRequest(entry: Seen, ids: Map<Request, string>): Promise<RecordedRequest | undefined> {
  const { request } = entry;
  const resourceType = request.resourceType();
  if (!keepRequest({ method: request.method(), url: request.url(), resourceType })) {
    return undefined;
  }
  const response = await request.response().catch(() => null);
  const responseHeaders: HeaderList = response === null
    ? []
    : (await response.headersArray()).map(({ name, value }) => [name, value]);
  const contentType = responseHeaders.find(([name]) => name.toLowerCase() === "content-type")?.[1] ?? "";
  const body = await entry.body;
  const record: RecordedRequest = {
    id: ids.get(request) ?? `r${entry.order}`,
    step: entry.step,
    method: request.method(),
    url: request.url(),
    headers: recordableHeaders((await request.headersArray()).map(({ name, value }) => [name, value])),
    resourceType: resourceType === "document" || resourceType === "xhr" || resourceType === "fetch" ? resourceType : "other",
    response: {
      status: response?.status() ?? 0,
      headers: recordableHeaders(responseHeaders),
      mimeType: contentType,
      ...(body === undefined ? {} : { body }),
    },
  };
  const postData = request.postData();
  if (postData !== null) {
    record.body = postData;
  }
  const previousHop = request.redirectedFrom();
  const previousId = previousHop === null ? undefined : ids.get(previousHop);
  if (previousId !== undefined) {
    record.redirectedFrom = previousId;
  }
  const origin = fetchOrigin(request);
  if (origin !== undefined) {
    record.fetchOrigin = origin;
  }
  return record;
}

/** A document is sent from its own origin; anything else from its page's. */
function fetchOrigin(request: Request): string | undefined {
  const own = httpOrigin(request.url());
  if (request.resourceType() === "document") {
    return own;
  }
  try {
    return httpOrigin(request.frame().url()) ?? own;
  } catch {
    // A service worker's request has no frame.
    return own;
  }
}

function httpOrigin(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.origin : undefined;
  } catch {
    return undefined;
  }
}
