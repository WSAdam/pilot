#!/usr/bin/env node
// pilot-mcp: with no command, an MCP server on stdio (Playwright MCP's tools
// plus task_start, task_finish and task_replay); `replay <name>` repeats a
// recorded task for cron and scripts; `list` names the recorded tasks.
// stdout carries the MCP protocol or the command's JSON; logs go to stderr.
import { homedir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

import { createPilotMcp, launchPilotContext, replayTask } from "./server.ts";
import { listRecordings } from "./store.ts";

const USAGE = `Usage:
  pilot-mcp [--pilot <bin>] [--user-data-dir <dir>] [--recordings <dir>] [--headless]
  pilot-mcp replay <task> [--input name=value]... [--allow-writes] [same options]
  pilot-mcp list [--recordings <dir>] [--user-data-dir <dir>]

--pilot defaults to $PILOT_BIN. Recordings live in <user-data-dir>/pilot-recordings,
or ~/.pilot/recordings without a profile.`;

async function main(): Promise<number> {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      pilot: { type: "string" },
      recordings: { type: "string" },
      "user-data-dir": { type: "string" },
      headless: { type: "boolean" },
      input: { type: "string", multiple: true },
      "allow-writes": { type: "boolean" },
      help: { type: "boolean" },
    },
  });
  if (values.help === true) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }
  const userDataDir = values["user-data-dir"];
  const recordingsDir =
    values.recordings ?? (userDataDir === undefined ? join(homedir(), ".pilot", "recordings") : join(userDataDir, "pilot-recordings"));
  const [command, taskName] = positionals;

  if (command === "list") {
    const names = await listRecordings(recordingsDir);
    process.stdout.write(names.length > 0 ? `${names.join("\n")}\n` : "");
    return 0;
  }

  const pilotBin = values.pilot ?? process.env.PILOT_BIN;
  if (pilotBin === undefined) {
    console.error(`❌ pilot-mcp needs --pilot <bin> or PILOT_BIN\n${USAGE}`);
    return 2;
  }
  const launch = {
    pilotBin,
    ...(values.headless === undefined ? {} : { headless: values.headless }),
    ...(userDataDir === undefined ? {} : { userDataDir }),
  };

  if (command === "replay") {
    if (taskName === undefined) {
      console.error(`❌ pilot-mcp replay needs a task name\n${USAGE}`);
      return 2;
    }
    const inputs = parseInputs(values.input ?? []);
    if (inputs === undefined) {
      console.error("❌ each --input must be name=value");
      return 2;
    }
    // A replay never needs a window.
    const pilot = await launchPilotContext({ ...launch, headless: true });
    try {
      const { ok, report } = await replayTask(pilot.context, recordingsDir, taskName, inputs, values["allow-writes"] === true);
      process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
      return ok ? 0 : 1;
    } catch (error) {
      process.stdout.write(`${JSON.stringify({ ok: false, reason: error instanceof Error ? error.message : String(error) }, null, 2)}\n`);
      return 1;
    } finally {
      await pilot.close();
    }
  }

  if (command !== undefined) {
    console.error(`❌ unknown command ${JSON.stringify(command)}\n${USAGE}`);
    return 2;
  }

  const pilotMcp = await createPilotMcp({ ...launch, recordingsDir });
  const transport = new StdioServerTransport();
  await pilotMcp.server.connect(transport);
  console.error(`✅ pilot-mcp serving MCP on stdio; recordings in ${recordingsDir}`);
  await new Promise<void>((done) => {
    const stop = (): void => done();
    process.stdin.once("end", stop);
    process.stdin.once("close", stop);
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
  });
  await pilotMcp.close();
  return 0;
}

function parseInputs(pairs: string[]): Record<string, string> | undefined {
  const inputs: Record<string, string> = {};
  for (const pair of pairs) {
    const equals = pair.indexOf("=");
    if (equals <= 0) {
      return undefined;
    }
    inputs[pair.slice(0, equals)] = pair.slice(equals + 1);
  }
  return inputs;
}

main().then(
  (code) => process.exit(code),
  (error: unknown) => {
    console.error(`❌ pilot-mcp: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}`);
    process.exit(1);
  },
);
