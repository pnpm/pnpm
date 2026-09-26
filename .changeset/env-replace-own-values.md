---
"@pnpm/config.env-replace": patch
"pnpm": patch
---

A `${VAR}` placeholder in `.npmrc` or `pnpm-workspace.yaml` whose name matches a built-in object property, such as `${toString}`, is now treated as an unset variable. It used to be replaced with the source text of a JavaScript function.
