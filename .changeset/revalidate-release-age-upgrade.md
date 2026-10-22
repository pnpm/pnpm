---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

With `minimumReleaseAge` set, pnpm now revalidates a package's cached full metadata with its ETag, so an unchanged registry answers with `304`. Before, if the registry did not list a publish time for every version, pnpm downloaded that metadata again whenever the cached copy was older than `minimumReleaseAge`.
