// Sends replayed requests through pilot: fetch() in a page on the request's
// origin, so pilot's cookie jar, Chrome TLS and HTTP/2, proxy and CORS all
// apply as they did when the task was recorded (docs/record-replay.md).
import type { BrowserContext, Page } from "playwright-core";

import type { HeaderList } from "./recording.ts";
import type { ConcreteRequest, ReplayResponse, ReplaySend } from "./replay.ts";

interface PageFetchResult {
  status: number;
  headers: HeaderList;
  mimeType: string;
  body: string;
  redirected: boolean;
  finalUrl: string;
}

export function pageSender(context: BrowserContext): { send: ReplaySend; close: () => Promise<void> } {
  const pages = new Map<string, Promise<Page>>();
  const pageFor = (origin: string): Promise<Page> => {
    let page = pages.get(origin);
    if (page === undefined) {
      page = (async () => {
        const opened = await context.newPage();
        // Any same-origin document will do; robots.txt is the cheapest to load.
        await opened.goto(`${origin}/robots.txt`).catch(() => undefined);
        return opened;
      })();
      pages.set(origin, page);
    }
    return page;
  };
  const send = async (request: ConcreteRequest): Promise<ReplayResponse> => {
    const origin = request.fetchOrigin ?? new URL(request.url).origin;
    const page = await pageFor(origin);
    const result: PageFetchResult = await page.evaluate(async ({ method, url, headers, body }) => {
      // The browser owns these; a page can't set them, and HTTP/2 pseudo-headers aren't headers.
      const browserOwned = /^(:|host$|connection$|content-length$|cookie$|origin$|referer$|user-agent$|accept-encoding$|sec-|priority$)/i;
      const init: RequestInit = {
        method,
        credentials: "include",
        redirect: "follow",
        headers: headers.filter(([name]) => !browserOwned.test(name)),
      };
      if (body !== undefined) {
        init.body = body;
      }
      const response = await fetch(url, init);
      return {
        status: response.status,
        headers: [...response.headers.entries()],
        mimeType: response.headers.get("content-type") ?? "",
        body: await response.text(),
        redirected: response.redirected,
        finalUrl: response.url,
      };
    }, { method: request.method, url: request.url, headers: request.headers, body: request.body });
    return result;
  };
  const close = async (): Promise<void> => {
    const opened = await Promise.all(pages.values());
    pages.clear();
    await Promise.all(opened.map((page) => page.close().catch(() => undefined)));
  };
  return { send, close };
}
