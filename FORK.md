# pilot fork of Moli

pilot is a fork of [lexmount/moli](https://github.com/lexmount/moli), made into a browser that [Playwright MCP](https://github.com/microsoft/playwright-mcp) can launch and drive as if it were Chromium. It runs headless, or headful with a "watch the agent" window. The deliberate choices are in [DECISIONS.md](DECISIONS.md); the CDP and launch contract pilot targets is in [docs/playwright-contract.md](docs/playwright-contract.md).

## Remotes

| Remote | URL | Use |
|---|---|---|
| `origin` | https://github.com/WSAdam/pilot | Our fork (renamed from `moli`). GitHub Actions are disabled here, because upstream workflows target self-hosted runners. Each finished unit is pushed here once its suite is green. |
| `upstream` | https://github.com/lexmount/moli | Fetch-only source of release tags. Its push URL is set to `DISABLED-pilot-never-pushes-to-moli`. |

**Base:** upstream main `51d6d1ee3` (2026-11-02), which is 113 commits past `v1.1.20`.

## Taking an upstream release

1. `git fetch upstream --tags`
2. `git merge <tag>` on `main`. Never rebase `main`: it is published.
3. Resolve conflicts using the tests as the oracle. Read both sides' tests, keep a resolution that satisfies both, and re-run the fixtures each `pilot-core` patch lists below.
4. Run `cargo fmt --all`, then `cargo clippy --workspace --all-targets --all-features -- -D warnings`, then `cargo nextest run --no-fail-fast`, then the pilot smoke groups. All of them must be green before the merge commit lands.
5. If upstream now fixes something a `pilot-core` patch covers, drop the patch and remove its row below.

## Fork-owned crates

| Crate | Purpose |
|---|---|
| `pilot` (planned, M1) | The `pilot` binary: a Chromium-compatible command line, pipe and port transports, and the choice between headless and the window |
| `pilot-window` (planned, M3) | The "watch the agent" window: winit + softbuffer, attached as a CDP client over loopback |

## Core patches (changes to upstream files)

Each patch is one commit whose subject starts with `pilot-core:`.

| Patch | Upstream files touched | Guarding test | Upstream equivalent |
|---|---|---|---|
| Accept negative CDP command ids, as Chromium does (Playwright closes a launched browser with `Browser.close` id −9999), and echo them back signed | `moli-protocol-cdp/src/{wire,lib}.rs`, `moli-protocol-server/src/cdp_frontend_router/{routing,routing/output}.rs` | `wire::tests::negative_command_ids_are_valid_like_chromium`, `routing::tests::negative_client_command_ids_are_accepted_and_restored_on_every_reply` | none |
