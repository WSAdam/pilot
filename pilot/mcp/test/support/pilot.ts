// Launches the pilot binary under test the way pilot-mcp does.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import type { Browser } from "playwright-core";

const here = fileURLToPath(new URL(".", import.meta.url));

/** PILOT_BIN, else the release build the gate downloads into target/release. */
export const pilotBin = process.env.PILOT_BIN ?? resolve(here, "../../../../target/release/pilot");

export function launchPilot(): Promise<Browser> {
  return chromium.launch({
    executablePath: pilotBin,
    headless: true,
    env: { ...process.env, PILOT_NO_WINDOW: "1" },
    timeout: 30_000,
  });
}
