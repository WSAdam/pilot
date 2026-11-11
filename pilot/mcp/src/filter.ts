// Which requests a recording keeps, and which headers it may store
// (docs/record-replay.md, "What gets recorded" and "Safety").
import type { HeaderList } from "./recording.ts";

export interface RawRequest {
  method: string;
  url: string;
  /** Playwright's resource type: document, xhr, fetch, image, script, ... */
  resourceType: string;
}

const KEPT_READ_TYPES = new Set(["document", "xhr", "fetch"]);
const READ_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Analytics, tag-manager, ad and telemetry hosts; a replay never needs them. */
const TRACKING_HOSTS = [
  "analytics.google.com",
  "browser-intake-datadoghq.com",
  "clarity.ms",
  "doubleclick.net",
  "google-analytics.com",
  "googletagmanager.com",
  "hotjar.com",
  "mixpanel.com",
  "amplitude.com",
  "nr-data.net",
  "segment.com",
  "segment.io",
  "sentry.io",
];

/** Tracking endpoints that live on a host with real pages. */
const TRACKING_PATHS: { host: string; pathPrefix: string }[] = [{ host: "facebook.com", pathPrefix: "/tr" }];

/** Cookie values come from the jar at replay and are never written to disk. */
const UNRECORDED_HEADERS = new Set(["cookie", "set-cookie"]);

export function keepRequest(request: RawRequest): boolean {
  const url = new URL(request.url);
  if (isTracking(url)) {
    return false;
  }
  if (!READ_METHODS.has(request.method.toUpperCase())) {
    return true;
  }
  return KEPT_READ_TYPES.has(request.resourceType);
}

export function recordableHeaders(headers: HeaderList): HeaderList {
  return headers.filter(([name]) => !UNRECORDED_HEADERS.has(name.toLowerCase()));
}

function isTracking(url: URL): boolean {
  const host = url.hostname.toLowerCase();
  if (TRACKING_HOSTS.some((tracker) => hostMatches(host, tracker))) {
    return true;
  }
  return TRACKING_PATHS.some(
    (tracker) => hostMatches(host, tracker.host) && url.pathname.startsWith(tracker.pathPrefix),
  );
}

function hostMatches(host: string, domain: string): boolean {
  return host === domain || host.endsWith(`.${domain}`);
}
