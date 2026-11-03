// A Deno project drives pilot the way it would drive Chrome: Playwright
// launches the binary by path. PILOT_BIN selects it (default: the release
// build).
import { assertEquals } from "@std/assert";
import { chromium } from "playwright-core";

const pilot = Deno.env.get("PILOT_BIN") ??
  new URL("../../target/release/pilot", import.meta.url).pathname;

const PAGE = `<!doctype html><title>deno</title>
<label>Name <input id=name></label>
<button onclick="document.getElementById('out').textContent = 'Hello, ' + document.getElementById('name').value">Greet</button>
<p id=out></p>`;

Deno.test("a Deno script launches pilot through Playwright and drives a page", async () => {
  const server = Deno.serve(
    { port: 0, hostname: "127.0.0.1", onListen() {} },
    () => new Response(PAGE, { headers: { "content-type": "text/html" } }),
  );
  const browser = await chromium.launch({
    executablePath: pilot,
    env: { ...Deno.env.toObject(), PILOT_NO_WINDOW: "1" },
    timeout: 30_000,
  });
  try {
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.addr.port}/`);
    await page.getByRole("textbox", { name: "Name" }).fill("Deno");
    await page.getByRole("button", { name: "Greet" }).click();
    assertEquals(await page.locator("#out").textContent(), "Hello, Deno");
  } finally {
    await browser.close();
    await server.shutdown();
  }
});
