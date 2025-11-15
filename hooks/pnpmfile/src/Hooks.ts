import {
  type PreResolutionHook,
  type Adapter,
} from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type BaseManifest } from '@pnpm/types'
import { type Log } from '@pnpm/core-loggers'
import { type CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'

export interface HookContext {
  log: (message: string) => void
}

export type ReadPackageHookFunction = <Pkg extends BaseManifest>(pkg: Pkg, context: HookContext) => Pkg | Promise<Pkg>

export interface Hooks {
  readPackage?: ReadPackageHookFunction
  preResolution?: PreResolutionHook
  afterAllResolved?: (lockfile: LockfileObject, context: HookContext) => LockfileObject | Promise<LockfileObject>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackageAsync
  fetchers?: CustomFetchers
  adapters?: Adapter[]
  /**
   * Hook to modify pnpm configuration.
   *
   * Note: The config parameter is actually the Config type from @pnpm/config,
   * but we use `any` here to avoid circular dependencies. Hook implementations
   * can safely cast it to the full Config type.
   *
   * @param config - The pnpm configuration object
   * @returns The modified configuration object
   */
  updateConfig?: (config: any) => any // eslint-disable-line @typescript-eslint/no-explicit-any
}
