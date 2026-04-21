# @pnpm/deps.compliance.license-resolver

## 1100.0.0

### Minor Changes

- 61952c2: `pnpm sbom` now detects licenses declared via the deprecated `licenses` array in `package.json` (e.g. `busboy`, `streamsearch`, `limiter`) and falls back to scanning on-disk `LICENSE` files — mirroring the resolution logic of `pnpm licenses`. Previously these packages were reported as `NOASSERTION`. Shared license resolution (manifest parsing + LICENSE-file fallback) lives in the new `@pnpm/deps.compliance.license-resolver` package. When a manifest sets both `license` and `licenses`, the modern `license` field now takes precedence for both commands (previously `pnpm licenses` preferred `licenses`) [#11248](https://github.com/pnpm/pnpm/issues/11248).
