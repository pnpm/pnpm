import {
  type PreResolutionHook,
  type ResolverPlugin,
} from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type BaseManifest, type HookContext } from '@pnpm/types'
import { type Log } from '@pnpm/core-loggers'
import { type CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'

export type ReadPackageHookFunction = <Pkg extends BaseManifest>(pkg: Pkg, context: HookContext) => Pkg | Promise<Pkg>

export interface Hooks {
  readPackage?: ReadPackageHookFunction
  preResolution?: PreResolutionHook
  afterAllResolved?: (lockfile: LockfileObject, context: HookContext) => LockfileObject | Promise<LockfileObject>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackageAsync
  fetchers?: CustomFetchers
  resolvers?: ResolverPlugin[]
  /**
   * Note: For a complete list of config keys, see the Config type in @pnpm/config.
   * We use { [key: string]: any } here to avoid circular dependencies.
   */
  updateConfig?: (config: { [key: string]: unknown }) => { [key: string]: unknown }
}
