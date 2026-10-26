## 1102.3.7

### Patch Changes

- The proxy settings `httpProxy`, `httpsProxy`, `noProxy`, `proxy`, and `noproxy` set in the global `config.yaml`, through `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, or as `--config.httpsProxy` on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. Previously, these requests read the proxy only from `.npmrc` and the `HTTPS_PROXY` family of variables [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).

- `strictSsl` and `localAddress` set in the global `config.yaml`, through `PNPM_CONFIG_STRICT_SSL` and `PNPM_CONFIG_LOCAL_ADDRESS`, or on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. These requests read them from `.npmrc` only before. `--config.localAddress` now reaches them in the camelCase spelling too [pnpm/tasks#201](https://github.com/pnpm/tasks/issues/201).

- Added the `pnpr` value to the `serverType` registry setting. pnpm leaves the tarball URLs of a pnpr registry out of `pnpm-lock.yaml`.

- When `https-proxy` is `false`, pnpm now downloads the pnpm version pinned by `packageManager` through the legacy `proxy` setting, as installs already do. Setting `proxy` to `false` now also stops that download from using the `HTTP_PROXY` environment variable. The signature check of the pinned pnpm now uses the same proxy and TLS settings as its download [pnpm/tasks#193](https://github.com/pnpm/tasks/issues/193).

- A `registries` entry whose `prefix` is `__proto__` is no longer ignored.

- The warning about unrecognized settings now marks `symlinkedLockfileTargetDirs` as a setting of a newer pnpm version.

- `pnpm install` now sizes its worker pool by the CPU quota of the container it runs in, such as a Kubernetes CPU limit, when that quota is smaller than the number of CPUs the container may run on. The default `workspaceConcurrency` respects the quota the same way. On a CI runner with a 4-CPU quota and hundreds of host CPUs, pnpm started hundreds of workers and ran out of memory [#17382](https://github.com/pnpm/pnpm/issues/17382).

- Updated dependencies:
  - @pnpm/hooks.pnpmfile@1100.0.38
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
  - @pnpm/workspace.workspace-manifest-reader@1100.3.1
