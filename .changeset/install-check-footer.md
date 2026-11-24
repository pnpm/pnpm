---
"pnpm": minor
---

An install lists the dependencies that don't support the current Node.js version or platform in the checks footer above `Done in`, such as `✗ 15 packages don't support Node.js 26.13.0: …`. It no longer prints a warning for each of these packages.

The `pnpm:install-check` log event now carries the error `code`, the `pkgId`, and, for an engine mismatch, the `wanted` and `current` versions.
