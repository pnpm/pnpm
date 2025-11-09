---
"pnpm": minor
---

Added a new setting: `trustPolicy`.

When set to `no-downgrade`, pnpm will fail installation if a package’s trust level has decreased compared to previous releases — for example, if it was previously published by a trusted publisher but now only has provenance or no trust evidence.
This helps prevent installing potentially compromised versions of a package.

Related issue: [#8889](https://github.com/pnpm/pnpm/issues/8889).
