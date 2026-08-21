---
"@pnpm/installing.modules-yaml": patch
"pnpm": patch
"pacquet": patch
---

Fixed pnpm failing to read `.modules.yaml` files containing long dependency paths [#13875](https://github.com/pnpm/pnpm/issues/13875). The manifest is now parsed as JSON (the format pnpm writes it in), falling back to the YAML parser only for manifests written by old pnpm versions.
