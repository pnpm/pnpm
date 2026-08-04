---
"@pnpm/config.commands": patch
"@pnpm/config.reader": patch
"pnpm": patch
---

A project's `pnpm-workspace.yaml` can no longer move the machine-level locations pnpm resolves for itself — among them `configDir`, which decided where `pnpm login` writes the granted token. pnpm now resolves these settings before it reads the project manifest, warns about the ones a manifest tried to set, and refuses to write them there with `pnpm config set` [#13629](https://github.com/pnpm/pnpm/issues/13629).
