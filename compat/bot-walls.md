# Bot walls

Which open-web sites still refuse pilot, which defense refuses it, and what that defense looks at. Measured 2026-11-04 from the same Mac and IP as `open-web.json`. By then pilot matched Chrome on every network fingerprint tls.peet.ws reports:

- JA4: `t13d1516h2_8daaf6152771_d8a2da3f94cd`
- Akamai HTTP/2: `1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p`
- a navigation's header order

To reproduce a site's first response, with no JavaScript run: `target/release/moli fetch --disable-js <url>`.

| Site | Status | Defense | What the first response is | Decided before any JavaScript ran? |
|---|---|---|---|---|
| eBay | 403 | Akamai Bot Manager (an edge reference id `0.<hex>.<time>.<hex>`, and a sensor script on a randomized path) | "Something went wrong on our end" | Yes |
| Reuters | 401 | DataDome (`captcha-delivery.com`) | `dd={rt:'c', t:'fe'}`: a captcha page, not the invisible device check (`rt:'i'`) | Yes |
| Etsy | 403 | DataDome | the same captcha page | Yes |
| Stack Overflow | 403 | Cloudflare (`challenge-platform`, `cType: 'precursor_interstitial'`) | "Just a moment...": a JavaScript challenge that fingerprints the browser and posts the result | No. pilot runs the challenge and fails it |

## What this says

- **eBay, Reuters and Etsy decide on the first request, from a fresh profile, before any script runs.** Only server-side signals reach them:
  - the TLS, HTTP/2 and header fingerprints, which now match Chrome;
  - the IP address's reputation;
  - the TCP/IP stack's fingerprint.
- **The browser claims the wrong operating system.** The User-Agent, `Sec-CH-UA-Platform` and `navigator.platform` say Windows (Moli's default identity, `moli-browser-profile`). The TCP stack (TTL 64, macOS's TCP options) says macOS. That mismatch is visible to all three vendors.
- **The IP may be part of it.** eBay loaded in both runs on 2026-11-03 and refused pilot after many automated visits from this IP. Plain curl gets 403 from eBay too.
- **Only Stack Overflow's wall tests the JavaScript engine.** Cloudflare's challenge probes a wide set of browser APIs (canvas, WebGL, audio, workers, timers, CSS) and pilot fails that probe.

## Proposed next layers, smallest first

1. **Identity that matches the host OS (small).** On a Mac, claim macOS in the User-Agent, `Sec-CH-UA-Platform` and `navigator.platform`. Do the same on Windows and Linux. That removes the TCP/IP mismatch. Then re-measure eBay, Reuters and Etsy.
2. **Comparison with a real Chrome (small, needs approval).** Load the same four sites in Chrome from the same IP. That separates IP reputation from pilot's fingerprint. Chromium runs only with `--use-mock-keychain --password-store=basic`, so macOS shows no Keychain prompt.
3. **JavaScript fingerprint layer for Cloudflare (large, uncertain).** This means WebGL with believable parameters, plus canvas and audio output that stay consistent with the claimed hardware, and whatever else the challenge probes. It is weeks of work, and stub values are themselves a common detection signal. Worth starting only if layers 1 and 2 show the remaining walls are pilot's fingerprint rather than the IP.
