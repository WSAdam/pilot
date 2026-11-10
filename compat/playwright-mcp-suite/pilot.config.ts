// pilot's run of Playwright's MCP suite. pilot comes from
// PLAYWRIGHT_MCP_EXECUTABLE_PATH, which the MCP server and the CLI daemon both
// read, so every test (MCP client or CLI) drives pilot. mcpArgs stays unset:
// the startClient fixture pushes each test's flags onto that array, and a
// shared array would carry one test's flags into the next. An empty
// mcpBrowser passes no --browser flag and makes the cdpServer fixture skip.
import { defineConfig } from '@playwright/test';
import base from './playwright.config';
import type { TestOptions } from './fixtures';

if (!process.env.PLAYWRIGHT_MCP_EXECUTABLE_PATH)
  throw new Error('set PLAYWRIGHT_MCP_EXECUTABLE_PATH to the pilot binary');
const chrome = base.projects!.find((project) => project.name === 'chrome')!;

export default defineConfig<TestOptions>({
  ...base,
  reporter: [['json', { outputFile: process.env.PILOT_RESULTS! }], ['list']],
  workers: Number(process.env.PILOT_WORKERS ?? 1),
  projects: [
    {
      name: 'pilot',
      testDir: chrome.testDir,
      metadata: { ...chrome.metadata, channel: 'pilot' },
      use: { mcpBrowser: '' },
    },
  ],
});
