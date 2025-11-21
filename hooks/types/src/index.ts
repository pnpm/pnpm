import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Resolution, type WantedDependency } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import { type Cafs } from '@pnpm/cafs-types'
import { type FetchOptions, type FetchResult, type Fetchers } from '@pnpm/fetcher-base'

// Custom resolution types must use scoped naming to avoid conflicts with pnpm's built-in types
export type CustomResolutionType = `@${string}/${string}`

// preResolution hook
export interface PreResolutionHookContext {
  wantedLockfile: LockfileObject
  currentLockfile: LockfileObject
  existsCurrentLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  lockfileDir: string
  storeDir: string
  registries: Registries
}

export interface PreResolutionHookLogger {
  info: (message: string) => void
  warn: (message: string) => void
}

export type PreResolutionHook = (ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) => Promise<void>

// Custom adapter hooks
export type { WantedDependency }

export interface ResolveOptions {
  lockfileDir: string
  projectDir: string
  preferredVersions: Record<string, string>
}

export interface ResolveResult {
  id: string
  resolution: Resolution
}

export interface Adapter {
  // Resolution phase: resolve package descriptors
  canResolve?: (wantedDependency: WantedDependency) => boolean | Promise<boolean>
  resolve?: (wantedDependency: WantedDependency, opts: ResolveOptions) => ResolveResult | Promise<ResolveResult>

  // Fetch phase: completely handle fetching for custom package types
  // This is a complete fetcher replacement, not just a resolution transformer
  // The fetchers parameter provides access to pnpm's standard fetchers for delegation
  canFetch?: (pkgId: string, resolution: Resolution) => boolean | Promise<boolean>
  fetch?: (cafs: Cafs, resolution: Resolution, opts: FetchOptions, fetchers: Fetchers) => FetchResult | Promise<FetchResult>

  // Force resolution: called for each dependency an adapter can resolve to determine if re-resolution is needed
  // If this returns true for any dependency, full resolution will be performed for all packages
  shouldForceResolve?: (wantedDependency: WantedDependency) => boolean | Promise<boolean>
}

export {
  getAdapterCacheKey,
  getCachedCanResolve,
  setCachedCanResolve,
  checkAdapterCanResolve,
} from './adapterCache.js'
