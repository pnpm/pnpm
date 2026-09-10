---
"pacquet": patch
---

`pnpm update <name>@<version>` now keeps the range operator the manifest already declares. Running `pnpm update react@19.3.0` on `"react": "^19.2.8"` writes `"react": "^19.3.0"` [#14745](https://github.com/pnpm/pnpm/issues/14745). A `jsr:` entry keeps its `jsr:` prefix as well, and a plain `pnpm update` now moves a `jsr:` range the way it moves an npm range.
