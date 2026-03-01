---
"pnpm": patch
---

Fixed an issue where `pnpm --version` and `pnpm --help` would fail with a packageManager mismatch error when the project's `package.json` specifies a different package manager or pnpm version.
