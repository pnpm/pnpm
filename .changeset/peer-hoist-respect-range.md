---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed peer dependency auto-install picking a version the peer range rejects. In a workspace with several projects, a package declaring a peer dependency with a semver range (for example `^1.0.0`) could get the highest version found anywhere in the workspace (for example a `2.0.0` resolved for another project) instead of a version that satisfies the range. Peers are now deduplicated onto the highest preferred version that satisfies the declared range, and when none does, the range is resolved from the registry.

Also fixed re-resolving with an existing lockfile hoisting a different peer version than a fresh install of the same manifest: root dependencies reused from the lockfile were invisible to peer hoisting, so a peer that a root dependency provides could be bound to another version.
