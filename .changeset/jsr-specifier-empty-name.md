---
"@pnpm/resolving.jsr-specifier-parser": patch
---

fix: throw `INVALID_JSR_PACKAGE_NAME` for `jsr:@scope/` specifiers with an empty package name instead of silently returning a malformed `@jsr/scope__` npm package name
