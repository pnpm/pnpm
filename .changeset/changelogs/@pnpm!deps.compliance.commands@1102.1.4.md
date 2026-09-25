## 1102.1.4

### Patch Changes

- `pnpm audit --fix` now prunes redundant overrides when one vulnerable range is a subset of another for the same package [#8577](https://github.com/pnpm/pnpm/issues/8577).

- `pnpm audit` and `pnpm audit signatures` now check only the dependencies of the projects selected by `--filter`, `--filter-prod`, or `--workspace-root`. The filter used to be ignored, so a filtered audit reported the whole workspace [#10982](https://github.com/pnpm/pnpm/issues/10982).

- `pnpm licenses list` run inside a workspace project now lists only that project's dependencies. Use `--recursive` or `--filter` to list the licenses of other workspace projects [#5689](https://github.com/pnpm/pnpm/issues/5689).

- `pnpm licenses list` failed or reported nothing in a workspace with `sharedWorkspaceLockfile: false`. It now reads the lockfile of each selected project [#10140](https://github.com/pnpm/pnpm/issues/10140).

- With `nodeLinker: hoisted`, `pnpm licenses list` reported paths under `node_modules/.pnpm` that do not exist. It now reports the directory where the hoisted linker placed each package [#8589](https://github.com/pnpm/pnpm/issues/8589).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm sbom` filtered to a single workspace project no longer replaces the project's own `license` or `bugs` field with the workspace root's value when the project's value is blank. The same applies to an `author`, `description`, `license`, `repository`, or `bugs` field set to `null` [#14882](https://github.com/pnpm/pnpm/issues/14882).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/config.writer@1100.0.28
  - @pnpm/deps.compliance.audit@1101.0.38
  - @pnpm/deps.compliance.license-scanner@1101.0.6
  - @pnpm/deps.compliance.sbom@1101.0.5
  - @pnpm/deps.security.signatures@1102.0.5
  - @pnpm/error@1100.2.0
  - @pnpm/installing.commands@1101.4.0
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
