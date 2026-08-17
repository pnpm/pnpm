## 1101.2.0

### Minor Changes

- Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).

### Patch Changes

- Updated dependencies:
  - @pnpm/config.reader@1101.17.0
  - @pnpm/crypto.shasums-file@1100.2.0
  - @pnpm/error@1100.1.2
