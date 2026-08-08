---
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.security.signatures": patch
"pnpm": patch
---

Fixed `pnpm audit signatures` silently dropping lockfile entries it could not resolve to a `packages:`/`snapshots:` entry: the `audited`/`verified` counts shrank with no entry in `missing` or `invalid`, and the command exited 0 [#13638](https://github.com/pnpm/pnpm/issues/13638). Such entries — for example from a lockfile edited or tampered with inconsistently — are now reported under `invalid`, so a corrupted lockfile can no longer pass `audit signatures` as clean.
