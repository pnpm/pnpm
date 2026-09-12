---
"@pnpm/config.normalize-registries": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/pnpr.client": major
"@pnpm/pnpr": minor
"pacquet": minor
"pnpm": minor
---

A pnpr resolve request now carries the client's registries the way the `registries` setting declares them — keyed by URL, with the scopes routed to each, the bare-specifier prefix each answers to, and each one's `serverType` — in place of the prefix map it used to send.

The server routes them through the same inversion the config reader runs, so a pnpr-served install resolves a scoped dependency from the registry that scope is routed to, which it previously could not: only the default registry and the prefix-addressed ones reached the server. A declared `serverType` reaches it too, so the tarball URLs pnpr omits from the lockfile match the ones the client reconstructs.

Built-in scope routes the project has not pointed elsewhere are not declared, so a pnpr server's allowlist is not asked about `npm.jsr.io` on requests that resolve no JSR package.

A registry a request only declares is no longer refused up front for being off the server's allowlist — a client describes its whole configuration, including scopes a given resolve never reaches, so a stray `@scope:registry` in a developer's `~/.npmrc` no longer fails every install against a pnpr server that does not serve it. The boundary moves to the fetch itself: an origin the resolve does reach is refused before the request leaves the server, with the same message.

This changes the resolve and verify-lockfile request bodies. A pnpr server and its clients have to be on matching versions; the protocol is still experimental and unversioned.
