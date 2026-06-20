---
"@pnpm/hooks.pnpmfile": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pnpm": minor
---

Added a `canonicalBinarySource` setting and a `getCanonicalBinaryPath` pnpmfile hook. When `canonicalBinarySource` is set to `"pnpmfile"` (in `pnpm-workspace.yaml`), pnpm consults the project's `getCanonicalBinaryPath` hook before running a command and re-execs into the returned on-disk pnpm binary. This lets an external tool pin the exact pnpm version a project must run under, without using the `packageManager` field of `package.json` — avoiding manifest churn and registry downloads. The hook receives the currently running pnpm version and returns `null` once it matches, which terminates the re-exec.
