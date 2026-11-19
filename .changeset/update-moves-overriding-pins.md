---
"pacquet": minor
---

`pnpm update --latest <pkg>` now moves the override that pins the named dependency [#8701](https://github.com/pnpm/pnpm/issues/8701). `pnpm update --interactive --latest` does the same for every selected dependency. An override keeps its own range shape: a pin on an exact version lands on the new exact version, and `^` stays `^`. When pnpm cannot update the governing override automatically, the update prints a warning naming it.
