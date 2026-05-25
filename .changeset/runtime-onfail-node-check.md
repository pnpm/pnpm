---
"@pnpm/engine.runtime.system-node-version": minor
"@pnpm/config.reader": patch
"pnpm": patch
---

Validate `devEngines.runtime` and `engines.runtime` version ranges for `node`, `deno`, and `bun` when `onFail` is set to `error` or `warn`. Previously these settings only had an effect with `onFail: 'download'` — the `error` and `warn` modes silently did nothing [#11818](https://github.com/pnpm/pnpm/issues/11818).
