---
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.compliance.sbom": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` now omits package author fields when the manifest author name is empty or contains only whitespace [pnpm/pnpm#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.
