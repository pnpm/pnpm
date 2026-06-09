---
"@pnpm/deps.security.signatures": minor
"@pnpm/installing.commands": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Security: pnpm now verifies the npm registry signature of a package-manager binary before spawning it, so a cloned repository cannot make pnpm download and execute an arbitrary native binary.

This covers two paths that select an executable from repository-controlled input:

- **pacquet install engine** — declaring `pacquet` (or `@pnpm/pacquet`) in `configDependencies` opts in to pnpm's Rust install engine. pnpm now verifies that the installed `pacquet` shim and the host's `@pacquet/<platform>-<arch>` binary carry a valid npm registry signature for their exact `name@version`, and refuses to run pacquet (failing the command) if the signature does not verify or cannot be checked. The only graceful fallback to pnpm's own engine is when pacquet has no binary for the current platform.
- **automatic version switch / `self-update`** — the `packageManager` / `devEngines.packageManager` field makes pnpm download and run a specific pnpm version. pnpm now verifies the registry signature of `pnpm`, `@pnpm/exe`, and the host platform binary before installing/spawning them, and refuses to run an engine whose signature does not match a published, signed release. The check runs only on an actual download (store cache miss), so it does not add a network round trip to every command.

In both cases the signature is verified over the *installed* integrity, against npm's public signing keys that ship embedded in the pnpm CLI (like corepack), so bytes substituted via a tampered lockfile or a repository-controlled registry fail verification — and a registry the user did not vouch for cannot supply its own signing keys. The signed packument is fetched from the configured registry, so an npm mirror works transparently. Verification fails closed: if it cannot be completed (for example, the registry is unreachable), the command fails rather than running an unverified binary. The embedded keys are kept current by a release-time check against npm's signing-keys endpoint.
