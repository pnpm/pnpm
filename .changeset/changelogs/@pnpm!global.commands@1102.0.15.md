## 1102.0.15

### Patch Changes

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.44
  - @pnpm/bins.remover@1100.0.34
  - @pnpm/bins.resolver@1100.0.22
  - @pnpm/cli.utils@1101.0.37
  - @pnpm/config.reader@1102.4.0
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/deps.inspection.list@1101.1.6
  - @pnpm/engine.runtime.node-resolver@1101.3.15
  - @pnpm/error@1100.2.3
  - @pnpm/global.packages@1101.1.13
  - @pnpm/installing.deps-installer@1104.2.12
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/store.connection-manager@1101.3.11
  - @pnpm/types@1102.1.5
