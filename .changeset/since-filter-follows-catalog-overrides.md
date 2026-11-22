---
"pacquet": patch
---

A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.
