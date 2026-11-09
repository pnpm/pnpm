---
"pacquet": minor
---

Added the experimental `lockfile.format: v13` setting. With it, pnpm writes `pnpm-lock.yaml` as JSON, indented with tabs, with a trailing comma after every entry. The file has one `packages` entry per package version in place of the `packages` and `snapshots` sections, and each entry records the versions of its peer dependencies as its properties. Updating a peer dependency then changes only the lines that record its version. pnpm reads lockfiles in both formats whatever the setting is.
