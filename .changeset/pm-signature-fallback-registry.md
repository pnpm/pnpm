---
"@pnpm/deps.security.signatures": minor
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

Registries that serve no npm signature metadata (private mirrors and feed proxies commonly strip `dist.signatures`) no longer break the automatic `packageManager` version switch and `pnpm self-update` [#13147](https://github.com/pnpm/pnpm/issues/13147). When the configured registry cannot provide a verifiable signature, pnpm now fetches the signature from `registry.npmjs.org` and verifies it against the same embedded npm keys over the installed integrity — which proves exactly the same thing. If no signature can be obtained from either source (for example, both are unreachable, or the registry publishes only a `shasum`), pnpm proceeds with a warning instead of failing, but only when the packages resolve through a registry configured in the user's own (non-project) configuration; the download stays pinned by the lockfile integrity, and a signature that exists but does not validate still fails the switch.
