# @pnpm/network.auth-header

## 1101.1.3

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1101.1.2

### Patch Changes

- 681b593: pnpm can now use different auth tokens for different package scopes, even when those scopes use the same registry URL.

  Previously, auth was selected only by registry URL. If `@org-a` and `@org-b` both used `https://npm.pkg.github.com/`, they had to share the same token. This caused problems for registries that issue tokens per organization or per scope.

  Configure a scope-specific token by adding the package scope after the registry URL in the auth key:

  ```ini
  @org-a:registry=https://npm.pkg.github.com/
  @org-b:registry=https://npm.pkg.github.com/

  //npm.pkg.github.com/:@org-a:_authToken=${ORG_A_TOKEN}
  //npm.pkg.github.com/:@org-b:_authToken=${ORG_B_TOKEN}

  //npm.pkg.github.com/:_authToken=${FALLBACK_TOKEN}
  ```

  `pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` writes the token to the same scope-specific auth key.

  When installing or publishing `@org-a/*`, pnpm uses `ORG_A_TOKEN`. For `@org-b/*`, pnpm uses `ORG_B_TOKEN`. Packages without a matching scope continue to use the registry-wide fallback token.

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
  - @pnpm/types@1101.3.2

## 1101.1.1

### Patch Changes

- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1

## 1101.1.0

### Minor Changes

- 5192edf: The pnpr install accelerator now forwards the caller's per-registry credentials on `POST /v1/install`, so it can resolve, verify, and fetch private dependencies from external registries as the caller. The client sends an `Authorization` header identifying itself to the pnpr server plus an `authHeaders` map of the registry tokens (built with `@pnpm/network.auth-header`), and the server threads those credentials through resolution and fetch instead of reaching the registry anonymously. Externally-resolved private content carries no pnpr access policy, so the server gates it per user against the owning registry — serving a cache hit only to a user the registry has cleared — and re-checks access (clearing it on a `401`/`403`) rather than letting the store's possession of the bytes authorize anyone. Packages the registry serves anonymously are classified public once (globally) and then served to everyone without per-user access checks, so a registry that mixes public and private packages doesn't pay the per-user cost for its public ones.

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0

## 1101.0.0

### Major Changes

- a23956e: Fix a credential disclosure issue where an unscoped `_authToken` (or `_auth`, or `username` + `_password`, or `tokenHelper`) defined in one source — `~/.npmrc`, `~/.config/pnpm/auth.ini`, a workspace `.npmrc`, CLI flags, etc. — would be sent as an `Authorization` header to whichever registry a different (potentially untrusted) source named. The same fix extends to client TLS credentials (`cert`, `key`) so they aren't presented to a registry their author didn't choose.

  pnpm now rewrites each unscoped per-registry setting (`_authToken`, `_auth`, `username`, `_password`, `tokenHelper`, `cert`, `key`) to its URL-scoped form at load time, using the `registry=` value declared in the same source (or the npmjs default registry if the source declares none). A later layer overriding `registry=` therefore cannot pull an unscoped credential along, because it is already pinned to the URL its author intended. `ca`/`cafile` are intentionally not rescoped — they're trust anchors, not credentials, and corporate MITM-proxy setups rely on them applying globally.

  Every rescope emits a deprecation warning telling the user where the setting was pinned and how to write it directly. npm has rejected unscoped credentials outright since `npm@9`, and pnpm intends to remove support in a future major release. To target a specific registry, write the setting URL-scoped (e.g. `//registry.example.com/:_authToken=...` or `//registry.example.com/:cert=...`).

  `@pnpm/network.auth-header`: removed the `defaultRegistry` parameter from `createGetAuthHeaderByURI` and `getAuthHeadersFromCreds`. Now that credentials are URL-scoped at load time, the merged `configByUri` never contains the empty-string "default registry" placeholder slot, so re-keying it onto the merged default registry is no longer needed.

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0

## 1100.0.3

### Patch Changes

- Updated dependencies [64afc92]
  - @pnpm/types@1101.1.1

## 1100.0.2

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- fb8962f: Fix `_password` handling for the default registry to decode from base64 before use, consistent with scoped registry behavior.
- b1ad9c7: Prepended `Bearer` to the authorization token generated by `tokenHelper` executable if it is missing, properly aligning pnpm's handling of token helpers with npm.
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/types@1001.0.0
  - @pnpm/error@1001.0.0

## 1000.0.6

### Patch Changes

- @pnpm/error@1000.0.5

## 1000.0.5

### Patch Changes

- @pnpm/error@1000.0.4

## 1000.0.4

### Patch Changes

- @pnpm/error@1000.0.3

## 1000.0.3

### Patch Changes

- 51bd373: Replace nerf-dart with @pnpm/config.nerf-dart to fix warning on Node.js 24.

## 1000.0.2

### Patch Changes

- @pnpm/error@1000.0.2

## 1000.0.1

### Patch Changes

- @pnpm/error@1000.0.1

## 3.0.3

### Patch Changes

- @pnpm/error@6.0.3

## 3.0.2

### Patch Changes

- @pnpm/error@6.0.2

## 3.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 3.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
  - @pnpm/error@6.0.0

## 2.2.0

### Minor Changes

- 5a5e42551: Export the loadToken function.

## 2.1.0

### Minor Changes

- 3ac0487b3: Add support for basic authorization header [#7371](https://github.com/pnpm/pnpm/issues/7371).

## 2.0.6

### Patch Changes

- 23039a6d6: Fix missing auth tokens in registries with paths specified (e.g. //npm.pkg.github.com/pnpm). #5970 #2933

## 2.0.5

### Patch Changes

- aa20818a0: Authorization token should be found in the configuration, when the requested URL is explicitly specified with a default port (443 on HTTPS or 80 on HTTP) [#6863](https://github.com/pnpm/pnpm/pull/6864).

## 2.0.4

### Patch Changes

- e44031e71: Improve the performance of searching for auth tokens.

## 2.0.3

### Patch Changes

- 4e7afec90: Ignore the port in the URL, while searching for authentication token in the `.npmrc` file [#6354](https://github.com/pnpm/pnpm/issues/6354).

## 2.0.2

### Patch Changes

- @pnpm/error@5.0.2

## 2.0.1

### Patch Changes

- @pnpm/error@5.0.1

## 2.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/error@5.0.0

## 1.0.1

### Patch Changes

- @pnpm/error@4.0.1

## 1.0.0

### Major Changes

- 804de211e: Initial release.
