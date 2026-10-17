---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

With `minimumReleaseAge` set, pnpm sends one conditional metadata request instead of two for a package whose cached metadata already holds the full document, but without every version's publish time.
