---
"@pnpm/cli.utils": major
"@pnpm/config.reader": major
"@pnpm/engine.pm.commands": major
"@pnpm/workspace.projects-reader": major
"pnpm": major
---

**Breaking:** removed the `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings. They existed only to derive the `onFail` behavior for the legacy `packageManager` field, and the `pmOnFail` setting introduced alongside `pnpm with` subsumes all three — it directly sets the `onFail` behavior of both `packageManager` and `devEngines.packageManager`. The `COREPACK_ENABLE_STRICT` environment variable is no longer honored (it only gated `packageManagerStrict`); use `pmOnFail` instead.

Migration:

| Removed setting                       | Replace with         |
| ------------------------------------- | -------------------- |
| `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
| `managePackageManagerVersions: false` | `pmOnFail: ignore`   |
| `packageManagerStrict: false`         | `pmOnFail: warn`     |
| `packageManagerStrictVersion: true`   | `pmOnFail: error`    |
| `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`     |
