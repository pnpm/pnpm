---
"@pnpm/config.commands": patch
"@pnpm/config.reader": patch
"pnpm": patch
---

A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials and its own installation — among those settings `configDir`, which decided where `pnpm login` writes the granted token. pnpm now ignores them in a project manifest and warns about the ones it found, and `pnpm config set` refuses to write them there [#13629](https://github.com/pnpm/pnpm/issues/13629).
