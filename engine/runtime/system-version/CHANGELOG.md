# @pnpm/engine.runtime.system-version

## 1100.0.0

### Minor Changes

- 35d2355: Validate `devEngines.runtime` and `engines.runtime` version ranges for `node`, `deno`, and `bun` when `onFail` is set to `error` or `warn`. Previously these settings only had an effect with `onFail: 'download'` — the `error` and `warn` modes silently did nothing [#11818](https://github.com/pnpm/pnpm/issues/11818). Violations now throw `ERR_PNPM_BAD_RUNTIME_VERSION`.

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0
  - @pnpm/cli.meta@1100.0.5
