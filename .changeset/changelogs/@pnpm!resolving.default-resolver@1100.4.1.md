## 1100.4.1

### Patch Changes

- Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).

- Updated dependencies:
  - @pnpm/engine.runtime.bun-resolver@1102.0.16
  - @pnpm/engine.runtime.deno-resolver@1102.0.16
  - @pnpm/engine.runtime.node-resolver@1101.2.0
  - @pnpm/error@1100.1.2
  - @pnpm/network.auth-header@1101.1.10
  - @pnpm/resolving.git-resolver@1100.1.17
  - @pnpm/resolving.local-resolver@1101.1.18
  - @pnpm/resolving.npm-resolver@1103.2.1
