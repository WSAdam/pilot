// Recordings on disk: one JSON file per task, readable only by its owner.
// They can hold response bodies with personal data, and are never logged
// (docs/record-replay.md, "Safety").
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import type { Recording } from "./recording.ts";

const TASK_NAME = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;

export async function saveRecording(dir: string, recording: Recording): Promise<string> {
  const path = recordingPath(dir, recording.name);
  await mkdir(dir, { recursive: true, mode: 0o700 });
  await writeFile(path, `${JSON.stringify(recording, null, 2)}\n`, { mode: 0o600 });
  console.error(`✅ pilot-mcp saved recording "${recording.name}"`);
  return path;
}

export async function loadRecording(dir: string, name: string): Promise<Recording> {
  const path = recordingPath(dir, name);
  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      throw new Error(`no recording named "${name}"`);
    }
    throw error;
  }
  return JSON.parse(text) as Recording;
}

export async function listRecordings(dir: string): Promise<string[]> {
  try {
    return (await readdir(dir))
      .filter((file) => file.endsWith(".json"))
      .map((file) => file.slice(0, -".json".length))
      .filter((name) => TASK_NAME.test(name))
      .sort();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return [];
    }
    throw error;
  }
}

function recordingPath(dir: string, name: string): string {
  if (!TASK_NAME.test(name)) {
    throw new Error(
      `task name ${JSON.stringify(name)} must be 1–64 letters, digits, "-" or "_", starting with a letter or digit`,
    );
  }
  return join(dir, `${name}.json`);
}
