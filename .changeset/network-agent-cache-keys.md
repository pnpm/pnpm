---
"@pnpm/network.agent": patch
"@pnpm/network.proxy-agent": patch
---

`getAgent` and `getProxyAgent` no longer return a cached agent created with different `strictSsl`, `maxSockets`, or `timeout` settings. An omitted `strictSsl` used to reuse an agent created with `strictSsl: false`, which skipped certificate verification.
