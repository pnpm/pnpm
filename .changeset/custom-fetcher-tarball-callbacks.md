---
"@pnpm/fetching.pick-fetcher": patch
"@pnpm/hooks.types": patch
"pnpm": patch
"pacquet": patch
---

Load configured pnpmfiles and provide native local and remote tarball callbacks to custom fetchers in the Rust CLI, including fresh installs that need to compute tarball integrity. Preserve locked integrity through custom-fetcher rewrites and reject unverified file maps in the Rust CLI.
