---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

`fetch-timeout` now limits how long a request may make no progress. The timer restarts on every chunk that arrives. A large download over a slow connection is no longer aborted while data is still coming in. A connection that stops delivering data still fails after `fetch-timeout` [#14604](https://github.com/pnpm/pnpm/issues/14604).
