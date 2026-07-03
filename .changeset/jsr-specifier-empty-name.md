---
"@pnpm/resolving.jsr-specifier-parser": patch
"pnpm": patch
---

pnpm now rejects `jsr:` specifiers whose package name is not a valid npm package name — an empty scope or name (e.g. `jsr:@scope/`), path separators inside the name, or any other shape `validate-npm-package-name` rejects — with `ERR_PNPM_INVALID_JSR_PACKAGE_NAME` instead of silently converting them into a malformed `@jsr/...` npm package name.
