---
"@pnpm/resolve-dependencies": patch
"@pnpm/default-reporter": patch
---

When encountering an external dependency using the `catalog:` protocol, a clearer error will be shown. Previously a confusing `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` error was thrown. The new error message will explain that the author of the dependency needs to run `pnpm publish` to replace the catalog protocol.
