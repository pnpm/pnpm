import { type PreResolutionHook } from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Log } from '@pnpm/core-loggers'
import { type CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'

export interface HookContext {
  log: (message: string) => void
}

export interface Hooks {
  // eslint-disable-next-line
  readPackage?: (pkg: any, context: HookContext) => any;
  preResolution?: PreResolutionHook
  afterAllResolved?: (lockfile: LockfileObject, context: HookContext) => LockfileObject | Promise<LockfileObject>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackageAsync
  fetchers?: CustomFetchers
}
