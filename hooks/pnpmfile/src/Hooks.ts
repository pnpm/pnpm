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
   * Resolve the canonical pnpm binary that this project must run under, given
   * the currently executing pnpm version. Consulted only when the
   * `canonicalBinarySource` setting is `"pnpmfile"`. Return the absolute path
   * to the canonical `pnpm` executable to re-exec into it, or `null`/`undefined`
   * to keep running the current binary (return `null` when the running version
   * already matches, which is how re-exec recursion terminates).
   */
  getCanonicalBinaryPath?: (
    context: GetCanonicalBinaryPathContext
  ) => string | null | undefined | Promise<string | null | undefined>
}

export interface GetCanonicalBinaryPathContext {
  /** Version of the pnpm that is currently executing, e.g. "11.0.4". */
  currentPnpmVersion: string
  /** Directory whose manifest/workspace opted in (the lockfile/project root). */
  rootDir: string
}
