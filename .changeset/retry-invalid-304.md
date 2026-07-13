---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Retry package metadata requests when a registry or proxy returns `304 Not Modified` to an unconditional request, preventing false `ERR_PNPM_CACHE_MISSING_AFTER_304` failures [pnpm/pnpm#12882](https://github.com/pnpm/pnpm/issues/12882).

If the retry also returns `304`, report `ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE` instead.
