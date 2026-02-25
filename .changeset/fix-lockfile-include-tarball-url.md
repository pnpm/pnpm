---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When `lockfile-include-tarball-url` is set to `false`, tarball URLs are now always excluded from the lockfile. Previously, tarball URLs could still appear for packages hosted under non-standard URLs, making the behavior flaky and inconsistent [#6667](https://github.com/pnpm/pnpm/issues/6667).
