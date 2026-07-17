---
"@pnpm/deps.peer-range": minor
"@pnpm/installing.deps-resolver": patch
"@pnpm/deps.inspection.peers-checker": patch
"pnpm": minor
"pacquet": minor
---

`peerDependencies` now accept dependency specifiers that carry a scheme — a named-registry spec (`<registry>:<version>`), an `npm:` alias, or a `file:`/git/URL spec — instead of rejecting them with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` [#13095](https://github.com/pnpm/pnpm/issues/13095). Such a peer is matched against the semver range carried by the specifier (`work:5.x.x` is checked as `5.x.x`, `npm:bar@^5` as `^5`), or against `*` when it carries no version, while the original specifier still selects the package to auto-install. Bare `name@version` values, which are almost always a mistake, are still rejected.
