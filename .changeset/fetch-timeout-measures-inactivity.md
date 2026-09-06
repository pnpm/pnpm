---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

`fetch-timeout` now limits how long a request may make no progress, so a large download over a slow connection is no longer aborted while it is still receiving data [#14604](https://github.com/pnpm/pnpm/issues/14604). The timer restarts on every chunk that arrives. A connection that stops delivering data still fails after `fetch-timeout`.
