// pilot-mcp: an MCP server in front of Playwright MCP. It launches pilot,
// owns the BrowserContext, hands that context to Playwright MCP, forwards
// every Playwright tool (each call is one recorded step), and adds
// task_start, task_finish and task_replay (docs/record-replay.md).
import playwrightMcp from "@playwright/mcp";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import type { CallToolResult, Tool } from "@modelcontextprotocol/sdk/types.js";
import { chromium } from "playwright-core";
import type { Browser, BrowserContext } from "playwright-core";

import { analyzeRecording } from "./analyze.ts";
import type { ReplayPlan } from "./recording.ts";
import { TaskRecorder } from "./recorder.ts";
import { replay } from "./replay.ts";
import { pageSender } from "./sender.ts";
import { loadRecording, saveRecording } from "./store.ts";

export interface PilotMcpOptions {
  pilotBin: string;
  recordingsDir: string;
  /** Without a window. Default false: pilot shows its "watch the agent" window. */
  headless?: boolean;
  /** A persistent profile, so logins survive restarts and CLI replays. */
  userDataDir?: string;
  /** Playwright MCP's own config (outputDir, snapshot, ...); never `browser`. */
  playwright?: Record<string, unknown>;
}

export interface PilotMcp {
  server: Server;
  close: () => Promise<void>;
}

const TASK_TOOLS: Tool[] = [
  {
    name: "task_start",
    description:
      "Start recording a task you'll want to repeat. Drive the browser as usual with the browser_* tools, then call task_finish. Later, task_replay repeats it in about a second without the browser steps.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Task name: letters, digits, '-' or '_', up to 64." },
        inputs: {
          type: "object",
          additionalProperties: { type: "string" },
          description: "Values a replay may change, by name (e.g. {\"month\": \"2026-10\"}). Text you type is an input too.",
        },
      },
      required: ["name"],
    },
  },
  {
    name: "task_finish",
    description:
      "Finish recording the task started with task_start. Returns what a replay will need: its inputs, the writes it repeats, and whether it can be replayed.",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "task_replay",
    description:
      "Repeat a recorded task through pilot's network stack, without the browser steps. Returns the task's result, or why the replay stopped; then run the task in the browser again with task_start and task_finish to refresh it.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string" },
        inputs: { type: "object", additionalProperties: { type: "string" } },
        allow_writes: {
          type: "boolean",
          description: "Required when the task repeats writes (POST, PUT, PATCH, DELETE): submitting forms, placing orders.",
        },
      },
      required: ["name"],
    },
  },
];

export interface PilotContext {
  context: BrowserContext;
  close: () => Promise<void>;
}

/** Launches pilot and returns the one context pilot-mcp drives and records. */
export async function launchPilotContext(
  options: Pick<PilotMcpOptions, "pilotBin" | "headless" | "userDataDir">,
): Promise<PilotContext> {
  console.error(`🚀 pilot-mcp starting pilot at ${options.pilotBin}`);
  const launch = {
    executablePath: options.pilotBin,
    headless: options.headless ?? false,
    env: options.headless === true ? { ...process.env, PILOT_NO_WINDOW: "1" } : { ...process.env },
    timeout: 30_000,
  };
  let browser: Browser | undefined;
  let context: BrowserContext;
  if (options.userDataDir !== undefined) {
    context = await chromium.launchPersistentContext(options.userDataDir, launch);
  } else {
    browser = await chromium.launch(launch);
    context = await browser.newContext();
  }
  const close = async (): Promise<void> => {
    await context.close().catch(() => undefined);
    await browser?.close().catch(() => undefined);
    console.error("✅ pilot-mcp closed pilot");
  };
  return { context, close };
}

export interface ReplayReport {
  ok: boolean;
  report: Record<string, unknown>;
}

/** Replays a saved task through pilot pages; shared by task_replay and the CLI. */
export async function replayTask(
  context: BrowserContext,
  recordingsDir: string,
  name: string,
  inputs: Record<string, string>,
  allowWrites: boolean,
): Promise<ReplayReport> {
  const recording = await loadRecording(recordingsDir, name);
  const plan = analyzeRecording(recording);
  const sender = pageSender(context);
  const started = performance.now();
  try {
    const outcome = await replay(recording, plan, sender.send, { inputs, allowWrites });
    const elapsedMs = Math.round(performance.now() - started);
    if (!outcome.ok) {
      console.error(`⚠️ pilot-mcp replay of "${name}" stopped: ${outcome.reason}`);
      return {
        ok: false,
        report: {
          ok: false,
          failedStep: outcome.failedStep,
          reason: outcome.reason,
          next: "Run the task in the browser again between task_start and task_finish to refresh its recording.",
        },
      };
    }
    console.error(`✅ pilot-mcp replayed "${name}" in ${elapsedMs} ms`);
    const result = outcome.result.map(({ url, contentType, text: body }) => ({ url, contentType, text: body }));
    return { ok: true, report: { ok: true, elapsedMs, result } };
  } finally {
    await sender.close();
  }
}

export async function createPilotMcp(options: PilotMcpOptions): Promise<PilotMcp> {
  const pilot = await launchPilotContext(options);
  const { context } = pilot;
  const recorder = new TaskRecorder(context);

  const upstreamServer = await playwrightMcp.createConnection(
    (options.playwright ?? {}) as Parameters<typeof playwrightMcp.createConnection>[0],
    async () => context,
  );
  const [upstreamClientTransport, upstreamServerTransport] = InMemoryTransport.createLinkedPair();
  await upstreamServer.connect(upstreamServerTransport);
  const upstream = new Client({ name: "pilot-mcp", version: "0.1.0" });
  await upstream.connect(upstreamClientTransport);

  const server = new Server({ name: "pilot-mcp", version: "0.1.0" }, { capabilities: { tools: {} } });
  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: [...(await upstream.listTools()).tools, ...TASK_TOOLS],
  }));
  server.setRequestHandler(CallToolRequestSchema, async (request): Promise<CallToolResult> => {
    const { name } = request.params;
    const args = (request.params.arguments ?? {}) as Record<string, unknown>;
    try {
      switch (name) {
        case "task_start":
          return taskStart(recorder, args);
        case "task_finish":
          return await taskFinish(recorder, context, options.recordingsDir);
        case "task_replay":
          return await taskReplay(recorder, context, options.recordingsDir, args);
        default:
          recorder.beginStep(name, args);
          return (await upstream.callTool({ name, arguments: args })) as CallToolResult;
      }
    } catch (error) {
      console.error(`❌ pilot-mcp ${name}: ${String(error)}`);
      return failure(error instanceof Error ? error.message : String(error));
    }
  });

  const close = async (): Promise<void> => {
    await upstream.close().catch(() => undefined);
    await pilot.close();
  };
  return { server, close };
}

function taskStart(recorder: TaskRecorder, args: Record<string, unknown>): CallToolResult {
  const name = args.name;
  if (typeof name !== "string") {
    return failure("task_start needs a name");
  }
  const inputs = stringRecord(args.inputs);
  recorder.start(name, inputs);
  return text(`Recording task "${name}". Drive the browser as usual, then call task_finish.`);
}

async function taskFinish(recorder: TaskRecorder, context: BrowserContext, recordingsDir: string): Promise<CallToolResult> {
  if (!recorder.active) {
    return failure("No task is being recorded; call task_start first.");
  }
  const page = context.pages().at(-1);
  const finalSnapshot = page === undefined ? undefined : await page.locator("body").ariaSnapshot().catch(() => undefined);
  const recording = await recorder.finish(finalSnapshot);
  await saveRecording(recordingsDir, recording);
  const plan = analyzeRecording(recording);
  return text(JSON.stringify(summary(recording.name, recording.steps.length, plan), null, 2));
}

async function taskReplay(
  recorder: TaskRecorder,
  context: BrowserContext,
  recordingsDir: string,
  args: Record<string, unknown>,
): Promise<CallToolResult> {
  if (recorder.active) {
    return failure("A task is being recorded; call task_finish before replaying, or the replay would be recorded too.");
  }
  const name = args.name;
  if (typeof name !== "string") {
    return failure("task_replay needs a name");
  }
  const { ok, report } = await replayTask(
    context,
    recordingsDir,
    name,
    stringRecord(args.inputs),
    args.allow_writes === true,
  );
  const body = JSON.stringify(report, null, 2);
  return ok ? text(body) : failure(body);
}

function summary(name: string, steps: number, plan: ReplayPlan): Record<string, unknown> {
  return {
    name,
    steps,
    requests: plan.requests.length,
    inputs: plan.inputs,
    writes: plan.writes.map((write) => `${write.method} ${write.url}`),
    replayable: plan.replayable,
    ...(plan.replayable ? {} : { notReplayableReasons: plan.notReplayableReasons }),
  };
}

function stringRecord(value: unknown): Record<string, string> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string"),
  );
}

function text(body: string): CallToolResult {
  return { content: [{ type: "text", text: body }] };
}

function failure(body: string): CallToolResult {
  return { content: [{ type: "text", text: body }], isError: true };
}
