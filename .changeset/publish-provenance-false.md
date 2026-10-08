---
"pacquet": patch
---

`pnpm publish --provenance=false` and `--no-provenance` now turn off provenance under trusted publishing, so a package can be published from a self-hosted runner. Setting `provenance: false` in `pnpm-workspace.yaml` does the same [#16721](https://github.com/pnpm/pnpm/issues/16721).
