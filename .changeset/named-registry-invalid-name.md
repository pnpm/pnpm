---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

pnpm now rejects named-registry specifiers (e.g. `gh:`) with a malformed package name — an empty scope (e.g. `gh:@/bar`), path separators inside the name (e.g. `gh:@scope/../name`), or a dot-segment name (e.g. `gh:..`) — with `ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME` instead of passing the name through to registry URLs and metadata cache file paths.
