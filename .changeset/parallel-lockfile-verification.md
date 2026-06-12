---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Sped up `pnpm install` with a frozen lockfile by running lockfile verification (the policy revalidation gate added for `minimumReleaseAge`/`trustPolicy` and the tarball-URL anti-tamper check) concurrently with fetching and linking instead of blocking the whole install on it. Dependency lifecycle scripts are still held back until verification succeeds, so no script runs on an unverified lockfile: if verification fails the install aborts before any dependency build, and if linking finishes first the install waits for the verification verdict before completing.
