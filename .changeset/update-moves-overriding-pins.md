---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --latest <pkg>` now moves the override that pins the named dependency, so the update takes effect instead of doing nothing [#8701](https://github.com/pnpm/pnpm/issues/8701). The interactive update does the same for every selected dependency. An override keeps its own range shape: a pin on an exact version lands on the new exact version, and `^` stays `^`. When pnpm cannot update the governing override automatically, the update prints a warning naming it.
