---
"@pnpm/os.env.path-extender-posix": patch
"pnpm": patch
"pacquet": patch
---

`pnpm setup` now writes a shell configuration that always adds the pnpm bin directory to the front of `PATH`. Sourcing that file again keeps the directory ahead of other entries already on `PATH`. Run `pnpm setup --force` to update a configuration written by an earlier version [pnpm/pnpm#7799](https://github.com/pnpm/pnpm/issues/7799).
