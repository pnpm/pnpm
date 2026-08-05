---
"pacquet": patch
---

Close three CLI parity gaps with the TypeScript pnpm CLI:

- `--registry <url>` is now accepted on every command as a universal rc-option, not only through `--config.registry=<url>` (`pnpm view pnpm dist-tags.latest --registry=https://registry.npmjs.org/`).
- `pnpm add` (and `pnpm add -g`) now accept `--allow-build=<pkg>`, appending the named packages to `allowBuilds` so they can run their lifecycle scripts during the install (`pnpm add @pnpm/exe@11.16.0 --allow-build=@pnpm/exe`).
- `--dir` / `-C` is now position-independent: it is accepted anywhere on the command line, before or after the subcommand (`pnpm add foo --dir /tmp/proj`).
