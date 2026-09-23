---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer expands environment variables in a `userAgent` set in a project's `pnpm-workspace.yaml`. A placeholder there could send a secret from the installer's environment to a registry chosen by the same file. A `userAgent` with a placeholder in that file is now ignored [#15415](https://github.com/pnpm/pnpm/issues/15415).
