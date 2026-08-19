---
"@pnpm/fetching.pick-fetcher": patch
"@pnpm/hooks.types": patch
"pnpm": patch
"pacquet": patch
---

Honor configured pnpmfiles in the Rust CLI and provide native local and remote tarball callbacks to custom fetchers. Preserve locked integrity when custom fetchers rewrite tarball resolutions, and reject unverified file maps in the Rust CLI.
