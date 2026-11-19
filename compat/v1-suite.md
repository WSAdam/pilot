# pilot v1

pilot v1 is the first version an agent can rely on to drive modern sites through Playwright MCP. It is done when all of these hold on a release build:

| Check | Bar | Where |
|---|---|---|
| Playwright and Playwright MCP launch pilot in every launch mode (launch, persistent, isolated, CDP endpoint; headless and headed, with the window) | all pass | `pilot/conformance` (`npm test`) |
| Every Playwright MCP tool does what it says on a fixture page | 100% | `compat/mcp-tools.json` |
| TodoMVC flows in every hosted framework example (React, Vue, Angular, Svelte, Preact, Lit, Ember, Backbone, jQuery, vanilla ES6, Knockout) | 100% | `compat/todomvc.json` |
| A live public site driven through MCP (the-internet.herokuapp.com: login, dialogs, uploads, dynamic content, drag and drop, new windows, frames). Chosen by the user on 2026-11-19 in place of their own apps | 100% of its 16 flows | `compat/live-site.json` (`npm run live-site`) |
| Public sites loaded, snapshotted and clicked through | tracked, not gated; bot walls are reported separately | `compat/open-web.json` |

The open-web corpus is the ladder beyond v1: its pass rate is recorded each release, with Gmail, Docs and YouTube as the north stars.
