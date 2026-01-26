import { type PreResolutionHook } from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Log } from '@pnpm/core-loggers'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'

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
}
