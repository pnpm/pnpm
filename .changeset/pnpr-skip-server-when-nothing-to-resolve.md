---
"pacquet": patch
---

With a configured `pnprServer`, `pnpm install` no longer pays a server exchange when there is nothing to resolve, so enabling pnpr is never slower than a direct install [pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904):

- The repeat-install "Already up to date" fast path now runs with a pnpr server configured.
- An install whose `pnpm-lock.yaml` still satisfies every manifest skips the server resolve exchange and materializes `node_modules` from the on-disk lockfile.
- The input-lockfile verification round trip is skipped when the local `lockfile-verified.jsonl` cache already covers the lockfile under the current policy; server-verified and server-resolved lockfiles are now recorded into that cache.
- Changing the `trustPolicy*`, `minimumReleaseAgeStrict`, or `minimumReleaseAgeExclude` settings now invalidates the repeat-install fast path, matching the TypeScript CLI's workspace-state check.
