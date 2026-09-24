---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`pnpm pack` now honors the `files` field of `package.yaml` and `package.json5` manifests. Git-hosted and injected local dependencies that use these manifests now honor it too [#7906](https://github.com/pnpm/pnpm/issues/7906).
