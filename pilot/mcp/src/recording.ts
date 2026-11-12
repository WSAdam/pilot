// The shapes pilot-mcp records during a task and the analyzer turns into a
// replay plan (docs/record-replay.md).

export type HeaderList = [name: string, value: string][];

/** One forwarded Playwright MCP tool call; each call is one step. */
export interface RecordedStep {
  index: number;
  tool: string;
  args: Record<string, unknown>;
}

export interface RecordedResponse {
  status: number;
  headers: HeaderList;
  mimeType: string;
  body?: string;
}

/** One request a step fired. `Cookie` is never recorded: the jar supplies it. */
export interface RecordedRequest {
  id: string;
  step: number;
  method: string;
  url: string;
  headers: HeaderList;
  body?: string;
  resourceType: "document" | "xhr" | "fetch" | "other";
  response: RecordedResponse;
  /**
   * The request this one is a redirect hop of. fetch() follows redirects, so a
   * hop is never sent on its own; it takes the chain's final response.
   */
  redirectedFrom?: string;
  /**
   * The origin a replay sends this request from: a document's own origin, or
   * the origin of the page that made an XHR or fetch, so CORS and SameSite
   * cookies behave as they did when it was recorded.
   */
  fetchOrigin?: string;
}

export interface Recording {
  name: string;
  /** Inputs named at task_start; their values become replay parameters. */
  inputs: Record<string, string>;
  steps: RecordedStep[];
  requests: RecordedRequest[];
  /** Text of the agent's last snapshot, used to find the result response. */
  finalSnapshot?: string;
}

/** Where a value sits in a request. */
export type SlotLocation =
  | { kind: "query"; name: string }
  | { kind: "path"; index: number }
  | { kind: "form"; name: string }
  | { kind: "json"; path: (string | number)[] }
  | { kind: "header"; name: string };

export type ExtractRule =
  | { in: "json"; path: (string | number)[] }
  | { in: "header"; name: string }
  | { in: "html-input"; name: string }
  | { in: "html-meta"; name: string }
  | { in: "text"; before: string; after: string };

/** Where a later run gets the value from. */
export type ValueSource =
  | { kind: "constant"; value: string }
  | { kind: "input"; name: string }
  | { kind: "extract"; fromRequest: string; rule: ExtractRule }
  | { kind: "regenerate"; format: "epoch-ms" | "epoch-s" | "uuid" }
  | { kind: "js-computed"; value: string };

export interface Slot {
  location: SlotLocation;
  recorded: string;
  source: ValueSource;
}

export interface PlannedRequest {
  id: string;
  step: number;
  method: string;
  url: string;
  slots: Slot[];
  /** Set for a redirect hop: it has no slots and is answered by its chain. */
  redirectedFrom?: string;
}

export interface PlannedWrite {
  id: string;
  step: number;
  method: string;
  url: string;
}

export interface ReplayPlan {
  name: string;
  /** Replay parameters with the values the recording used. */
  inputs: Record<string, string>;
  requests: PlannedRequest[];
  writes: PlannedWrite[];
  /** The requests whose responses hold the answer the agent read, in recorded order. */
  resultRequests: string[];
  replayable: boolean;
  notReplayableReasons: string[];
}
