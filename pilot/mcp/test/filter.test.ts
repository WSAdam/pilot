// Record→replay filter: a recording keeps documents, XHR, fetch and every
// write, drops assets and analytics, and never stores a cookie value
// (docs/record-replay.md, "What gets recorded" and "Safety"). 2026-11-11.
import { test } from "node:test";
import assert from "node:assert/strict";

import { keepRequest, recordableHeaders } from "../src/filter.ts";

test("documents, XHR and fetch requests are kept", () => {
  for (const resourceType of ["document", "xhr", "fetch"]) {
    assert.equal(keepRequest({ method: "GET", url: "https://shop.test/a", resourceType }), true, resourceType);
  }
});

test("images, stylesheets, fonts, scripts and media are dropped", () => {
  for (const resourceType of ["image", "stylesheet", "font", "script", "media", "manifest", "texttrack"]) {
    assert.equal(keepRequest({ method: "GET", url: "https://shop.test/a", resourceType }), false, resourceType);
  }
});

test("a write is kept whatever its resource type", () => {
  assert.equal(keepRequest({ method: "POST", url: "https://shop.test/upload", resourceType: "other" }), true);
});

test("analytics and ad hosts are dropped, even for writes", () => {
  for (const url of [
    "https://www.google-analytics.com/g/collect?v=2",
    "https://region1.analytics.google.com/g/collect",
    "https://www.googletagmanager.com/gtm.js?id=GTM-X",
    "https://stats.g.doubleclick.net/j/collect",
    "https://www.facebook.com/tr/?id=1",
    "https://api.segment.io/v1/t",
    "https://o1.ingest.sentry.io/api/2/envelope/",
    "https://bam.nr-data.net/events/1",
  ]) {
    assert.equal(keepRequest({ method: "POST", url, resourceType: "fetch" }), false, url);
  }
});

test("cookie values are never recorded: Cookie and Set-Cookie headers are dropped", () => {
  assert.deepEqual(
    recordableHeaders([
      ["Cookie", "session=abc"],
      ["X-CSRF-Token", "tok-1"],
      ["set-cookie", "session=def; HttpOnly"],
      ["Content-Type", "application/json"],
    ]),
    [
      ["X-CSRF-Token", "tok-1"],
      ["Content-Type", "application/json"],
    ],
  );
});
