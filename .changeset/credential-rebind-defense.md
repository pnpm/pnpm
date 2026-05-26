---
"@pnpm/config.reader": patch
"@pnpm/network.auth-header": major
"pnpm": patch
---

Fix a credential disclosure issue where an unscoped `_authToken` (or `_auth`, or `username` + `_password`, or `tokenHelper`) defined in one source — `~/.npmrc`, `~/.config/pnpm/auth.ini`, a workspace `.npmrc`, CLI flags, etc. — would be sent as an `Authorization` header to whichever registry a different (potentially untrusted) source named. The same fix extends to client TLS credentials (`cert`, `key`) so they aren't presented to a registry their author didn't choose.

pnpm now rewrites each unscoped per-registry setting (`_authToken`, `_auth`, `username`, `_password`, `tokenHelper`, `cert`, `key`) to its URL-scoped form at load time, using the `registry=` value declared in the same source (or the npmjs default registry if the source declares none). A later layer overriding `registry=` therefore cannot pull an unscoped credential along, because it is already pinned to the URL its author intended. `ca`/`cafile` are intentionally not rescoped — they're trust anchors, not credentials, and corporate MITM-proxy setups rely on them applying globally.

Every rescope emits a deprecation warning telling the user where the setting was pinned and how to write it directly. npm has rejected unscoped credentials outright since `npm@9`, and pnpm intends to remove support in a future major release. To target a specific registry, write the setting URL-scoped (e.g. `//registry.example.com/:_authToken=...` or `//registry.example.com/:cert=...`).

`@pnpm/network.auth-header`: removed the `defaultRegistry` parameter from `createGetAuthHeaderByURI` and `getAuthHeadersFromCreds`. Now that credentials are URL-scoped at load time, the merged `configByUri` never contains the empty-string "default registry" placeholder slot, so re-keying it onto the merged default registry is no longer needed.
