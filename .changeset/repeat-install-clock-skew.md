---
"pacquet": patch
---

Fixed the incremental install fast path wrongly reporting "already up to date" — skipping re-resolution — when a `package.json`, `.pnpmfile.cjs`, or patch file was edited immediately after an install. The freshness check compared file modification times against a wall-clock timestamp, which broke in two ways: on a machine whose wall clock and filesystem clock disagree (seen on some CI runners) the timestamp could sit ahead of a later edit's mtime, and a fast install could write its lockfile in the same millisecond as the subsequent edit. The check now records the baseline from filesystem mtimes and compares at nanosecond precision.
