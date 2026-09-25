---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm version <bump>` with `--dry-run` no longer edits `package.json` files. It now only reports the bumps it would make, and skips the working tree check, the version lifecycle scripts, the commit, and the tag [`pnpm/pnpm#13953`](https://github.com/pnpm/pnpm/issues/13953).
