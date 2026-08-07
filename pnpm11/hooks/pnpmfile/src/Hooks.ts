import type { Log } from '@pnpm/core-loggers'
import type { PreResolutionHook } from '@pnpm/hooks.types'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ImportIndexedPackageAsync } from '@pnpm/store.controller-types'

export interface HookContext {
  log: (message: string) => void
}

export interface Hooks {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Flexible hook signature for any package manifest
  readPackage?: (pkg: any, context: HookContext) => any
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Flexible hook signature for any package manifest
  beforePacking?: (pkg: any, dir: string, context: HookContext) => any
  preResolution?: PreResolutionHook
  afterAllResolved?: (lockfile: LockfileObject, context: HookContext) => LockfileObject | Promise<LockfileObject>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackageAsync
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Flexible hook signature for any config object
  updateConfig?: (config: any) => any
  /**
   * Returns a fingerprint of any external state that the pnpmfile's behavior
   * depends on (for example, the state of a custom package source used by a
   * custom resolver). The value is recorded in the workspace state file and
   * compared by the up-to-date checks used by `verify-deps-before-run` and
   * `optimistic-repeat-install`; when it changes, `node_modules` is
   * considered outdated even if the lockfile and manifests are unchanged.
   *
   * The fingerprint is never written to the lockfile, so it may be
   * machine-specific. The hook runs on every up-to-date check (before
   * `pnpm run`/`pnpm exec` under `verify-deps-before-run`, and on repeat
   * installs under `optimistic-repeat-install`) as well as after every
   * install to record the value, so it should be cheap to compute.
   */
  calculateFingerprint?: () => string | Promise<string>
}
