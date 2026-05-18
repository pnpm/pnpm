---
"@pnpm/core-loggers": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": minor
"pnpm": patch
---

The lockfile verifier added in #11705 now emits `pnpm:lockfile-verification` log events (`status: 'started' | 'done'`) around the registry round-trip pass, and the default reporter renders them as a transient progress line so users can see that pnpm is doing work — on a cold registry cache the round-trip can take a noticeable beat, and the previous behavior was complete silence followed by either a long pause or an error. The cached short-circuit stays silent (no logs when no work happens), and the `done` line carries the number of distinct entries that were checked plus the elapsed time.

Pacquet parity: not ported — pacquet doesn't carry the lockfile verifier yet (see the parity note on #11705).
