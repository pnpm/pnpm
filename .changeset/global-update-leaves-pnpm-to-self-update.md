---
"@pnpm/cli.default-reporter": patch
"@pnpm/global.commands": minor
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update -g` no longer touches pnpm's own global install. It used to reinstall it from the `latest` dist-tag and relink the pnpm home's bins, silently rolling a `pnpm self-update`-installed version back and then failing with a bin conflict; use `pnpm self-update` to change the pnpm version [#14270](https://github.com/pnpm/pnpm/issues/14270).

The progress output no longer overwrites lines above it once it grows taller than the terminal window [#14270](https://github.com/pnpm/pnpm/issues/14270).
