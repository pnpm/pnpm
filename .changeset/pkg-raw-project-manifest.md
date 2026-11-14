---
"@pnpm/cli.utils": patch
"@pnpm/pkg-manifest.commands": patch
"@pnpm/workspace.project-manifest-reader": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.
