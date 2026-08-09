---
"@pnpm/deps.compliance.audit": minor
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.security.signatures": minor
"pnpm": patch
---

Fixed `pnpm audit signatures` silently dropping lockfile entries it could not resolve to a `packages:`/`snapshots:` entry: the `audited`/`verified` counts shrank with no entry in `missing` or `invalid`, and the command exited 0 [#13638](https://github.com/pnpm/pnpm/issues/13638). Such entries — for example from a lockfile edited or tampered with inconsistently — are now reported under `invalid`, so a corrupted lockfile can no longer pass `audit signatures` as clean.

**Known gap:** the Rust `pnpm/` CLI's audit-signatures request builder (`crates/cli/src/cli_args/audit/`) is not covered by this change and still silently drops the same kind of unresolvable entries. Parity for that path is deferred to a follow-up PR rather than attempted here without adequate familiarity with that codebase to verify correctness.
