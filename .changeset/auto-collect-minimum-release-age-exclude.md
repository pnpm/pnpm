---
"@pnpm/store.controller-types": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Tightened the `minimumReleaseAge` story so the bypass becomes explicit on disk instead of silent, and removed the discover-by-loop dance for strict-mode users:

1. Fresh resolutions in loose mode (`minimumReleaseAgeStrict: false`) that fall back to a version newer than the cutoff auto-collect the picked `name@version` into the workspace manifest's `minimumReleaseAgeExclude`. A single info message lists the additions; entries already on the list are left alone.
2. The post-resolution lockfile verifier introduced in #11583 now runs in loose mode too — every accepted-immature pin must be on `minimumReleaseAgeExclude`, just like strict mode requires. A lockfile produced under a weaker (or absent) policy that still has immature entries is rejected the same way strict mode would reject it.
3. **Strict mode (interactive)** no longer aborts on the first immature pick. The resolver gathers every immature direct *and* transitive in one pass; before peer-dependency resolution runs, pnpm prompts the user with the full list and asks whether to add them all to `minimumReleaseAgeExclude` and proceed. Approve → install continues and the workspace manifest is written at the end. Decline → resolution aborts before the lockfile or package.json is touched (tarballs already in the store stay, since the store is idempotent). This closes the [#10488](https://github.com/pnpm/pnpm/issues/10488) loop where security bumps to packages with platform-specific transitives (e.g. `next` + the `@next/swc-*` shims) made users re-run `pnpm add` once per transitive.
4. **Strict mode (non-interactive / CI)** is unchanged: the resolver still throws `ERR_PNPM_NO_MATURE_MATCHING_VERSION` on the first immature pick, so deterministic CI behavior is preserved. The expected workflow is interactive approval locally → the lockfile + workspace manifest get committed → CI runs cleanly against the populated exclude list.
