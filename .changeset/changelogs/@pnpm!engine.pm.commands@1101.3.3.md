## 1101.3.3

### Patch Changes

- Registries that serve no npm signature metadata (private mirrors and feed proxies commonly strip `dist.signatures`) no longer break the automatic `packageManager` version switch and `pnpm self-update` [#13147](https://github.com/pnpm/pnpm/issues/13147). When the configured registry cannot provide a verifiable signature, pnpm now fetches the signature from `registry.npmjs.org` and verifies it against the same embedded npm keys over the installed integrity — which proves exactly the same thing. If no signature can be obtained from either source (for example, both are unreachable, or the registry publishes only a `shasum`), pnpm proceeds with a warning instead of failing, but only when the packages resolve through a registry configured in the user's own (non-project) configuration; the download stays pinned by the lockfile integrity, and a signature that exists but does not validate still fails the switch.

- `pnpm setup` no longer makes Node.js print a `MODULE_TYPELESS_PACKAGE_JSON` warning about `dist/worker.js` on every command. The `package.json` it writes next to a standalone executable now declares `"type": "module"`.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.26
  - @pnpm/config.reader@1101.16.1
  - @pnpm/deps.graph-hasher@1100.2.16
  - @pnpm/deps.security.signatures@1101.3.0
  - @pnpm/global.commands@1100.1.0
  - @pnpm/global.packages@1100.0.17
  - @pnpm/installing.client@1100.3.3
  - @pnpm/installing.deps-restorer@1102.3.1
  - @pnpm/installing.env-installer@1102.0.16
  - @pnpm/lockfile.fs@1100.2.1
  - @pnpm/resolving.npm-resolver@1103.2.0
  - @pnpm/store.connection-manager@1100.3.16
