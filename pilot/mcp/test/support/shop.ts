// A small shop for the pilot-backed tests: a login cookie, an order form
// guarded by a per-load CSRF token, a 302 to the new order's page, and a JSON
// list of orders. It logs what reached it, so tests can check the wire.
import { randomBytes } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

export interface ShopHit {
  method: string;
  path: string;
  hasSession: boolean;
  secFetchMode: string | undefined;
  body: string;
}

export interface ShopOrder {
  id: number;
  item: string;
  qty: number;
}

export interface Shop {
  base: string;
  hits: ShopHit[];
  orders: () => ShopOrder[];
  close: () => Promise<void>;
}

export async function startShop(): Promise<Shop> {
  const sessions = new Map<string, { token: string; orders: ShopOrder[] }>();
  const hits: ShopHit[] = [];
  let nextOrder = 100;
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://shop");
    const cookie = /session=(\w+)/.exec(request.headers.cookie ?? "")?.[1];
    const session = cookie === undefined ? undefined : sessions.get(cookie);
    let body = "";
    for await (const chunk of request) {
      body += String(chunk);
    }
    const secFetchMode = request.headers["sec-fetch-mode"];
    hits.push({
      method: request.method ?? "GET",
      path: url.pathname,
      hasSession: session !== undefined,
      secFetchMode: typeof secFetchMode === "string" ? secFetchMode : undefined,
      body,
    });
    const html = (status: number, markup: string, headers: Record<string, string> = {}): void => {
      response.writeHead(status, { "content-type": "text/html; charset=utf-8", ...headers });
      response.end(
        `<!doctype html><html><head><title>Shop</title><link rel="stylesheet" href="/site.css"></head>` +
          `<body><nav>Home Orders Account</nav>${markup}</body></html>`,
      );
    };
    if (url.pathname === "/site.css") {
      response.writeHead(200, { "content-type": "text/css" });
      response.end("nav { font-weight: bold }");
      return;
    }
    if (url.pathname === "/logo.png") {
      response.writeHead(200, { "content-type": "image/png" });
      response.end(Buffer.from("89504e470d0a1a0a", "hex"));
      return;
    }
    if (url.pathname === "/robots.txt") {
      response.writeHead(200, { "content-type": "text/plain" });
      response.end("User-agent: *\n");
      return;
    }
    if (url.pathname === "/login") {
      const id = randomBytes(8).toString("hex");
      sessions.set(id, { token: "", orders: [] });
      html(200, '<a href="/orders/new">New order</a>', {
        "set-cookie": `session=${id}; Path=/; HttpOnly; SameSite=Lax`,
      });
      return;
    }
    if (session === undefined) {
      html(401, "<p>Please sign in</p>");
      return;
    }
    if (request.method === "GET" && url.pathname === "/orders/new") {
      session.token = randomBytes(16).toString("base64url");
      html(
        200,
        `<h1>New order</h1><img src="/logo.png" alt="logo"><form method="post" action="/orders">` +
          `<input type="hidden" name="csrf" value="${session.token}">` +
          `<label>Item <input name="item"></label><label>Qty <input name="qty"></label>` +
          `<button type="submit">Place order</button></form>`,
      );
      return;
    }
    if (request.method === "POST" && url.pathname === "/orders") {
      const form = new URLSearchParams(body);
      if (form.get("csrf") !== session.token) {
        html(403, "<p>Bad token</p>");
        return;
      }
      const order = { id: nextOrder, item: form.get("item") ?? "", qty: Number(form.get("qty")) };
      nextOrder += 1;
      session.orders.push(order);
      response.writeHead(302, { location: `/orders/${order.id}` });
      response.end();
      return;
    }
    const orderMatch = /^\/orders\/(\d+)$/.exec(url.pathname);
    if (orderMatch !== null) {
      const order = session.orders.find((entry) => entry.id === Number(orderMatch[1]));
      if (order === undefined) {
        html(404, "<p>No such order</p>");
      } else {
        html(200, `<h1>Order ${order.id}</h1><p>${order.item} x ${order.qty}</p>`);
      }
      return;
    }
    if (url.pathname === "/api/orders") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ orders: session.orders, total: session.orders.length }));
      return;
    }
    html(404, "<p>Not found</p>");
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address() as AddressInfo;
  return {
    base: `http://127.0.0.1:${port}`,
    hits,
    orders: () => [...sessions.values()].flatMap((entry) => entry.orders),
    close: () => new Promise<void>((done) => server.close(() => done())),
  };
}
