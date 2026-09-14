## 1100.0.43

### Patch Changes

- Registries that share a host but differ by URL path — one JFrog Artifactory, Nexus, AWS CodeArtifact or GitLab Packages instance serving several repositories — now get a metadata cache directory each. Previously they shared one, so resolving a package from one of them could answer with another's versions, integrity hashes and tarball URLs and fail with `ERR_PNPM_TARBALL_URL_MISMATCH` [#13558](https://github.com/pnpm/pnpm/issues/13558).

  The URL scheme is part of the cache directory name too, so an `http` registry can no longer hand its metadata — which can be rewritten in transit — to a resolution configured for `https` at the same host.

  The first install after upgrading refetches registry metadata once. The package store is untouched.

  `pnpm cache view` now labels each entry with the full registry URL. It printed `registry.npmjs.org` before and prints `https://registry.npmjs.org/` now.

  `pnpm cache list-registries` and `pnpm cache list` print the new directory names. Scripts that parse either command need updating.

- Updated dependencies:
  - @pnpm/config.reader@1102.2.0
  - @pnpm/resolving.npm-resolver@1104.1.2
  - @pnpm/store.cafs@1100.3.2
