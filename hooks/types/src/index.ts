import { type LockfileObject, type PackageSnapshot } from '@pnpm/lockfile.types'
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

// Custom resolver and fetcher hooks
export type { WantedDependency }

export interface ResolveOptions {
  lockfileDir: string
  projectDir: string
  preferredVersions: Record<string, string>
  currentPkg?: {
    id: string
    resolution: Resolution
    name?: string
    version?: string
  }
}

export interface ResolveResult {
  id: string
  resolution: Resolution
}

export interface CustomResolver {
  /**
   * Called during resolution to determine if this resolver should handle a dependency.
   * This should be a cheap check (ideally synchronous) as it's called for every dependency.
   *
   * @param wantedDependency - The dependency descriptor to check
   * @returns true if this resolver should handle the dependency
   */
  canResolve?: (wantedDependency: WantedDependency) => boolean | Promise<boolean>

  /**
   * Called to resolve a dependency that canResolve returned true for.
   * This can be an expensive async operation (e.g., network requests).
   *
   * @param wantedDependency - The dependency descriptor to resolve
   * @param opts - Resolution options including lockfileDir, projectDir, and preferredVersions
   * @returns Resolution result with id and resolution object
   */
  resolve?: (wantedDependency: WantedDependency, opts: ResolveOptions) => ResolveResult | Promise<ResolveResult>

  /**
   * Called on subsequent installs (when lockfile exists) to determine if this dependency
   * needs re-resolution. This is called before resolution, so the original specifier
   * from package.json is not available — use depPath and pkgSnapshot to decide.
   *
   * This hook is called independently of canResolve. It is invoked for every package
   * in the lockfile, regardless of whether canResolve would match. Resolvers should
   * handle their own filtering (e.g., by inspecting depPath or pkgSnapshot.resolution).
   *
   * If this returns true for ANY dependency, full resolution will be triggered for ALL packages,
   * bypassing the "Lockfile is up to date" optimization.
   *
   * Use this to implement custom cache invalidation logic (e.g., time-based expiry, version checks).
   *
   * @param depPath - The dependency path (e.g., 'lodash@4.17.21' or '@scope/pkg@1.0.0')
   * @param pkgSnapshot - The lockfile entry for this dependency
   * @returns true to force re-resolution of all dependencies
   */
  shouldRefreshResolution?: (depPath: string, pkgSnapshot: PackageSnapshot) => boolean | Promise<boolean>
}

export interface CustomFetcher {
  /**
   * Called to determine if this fetcher should handle fetching a package.
   * This is called for each package that needs to be fetched.
   *
   * @param pkgId - The package ID (e.g., 'foo@1.0.0' or custom format)
   * @param resolution - The resolution object from the lockfile
   * @returns true if this fetcher should handle fetching this package
   */
  canFetch?: (pkgId: string, resolution: Resolution) => boolean | Promise<boolean>

  /**
   * Called to fetch and extract a package's contents.
   * This is a complete fetcher implementation that should download/copy the package
   * and add its files to the content-addressable file system (cafs).
   *
   * The fetchers parameter provides access to pnpm's standard fetchers, allowing you
   * to delegate to them (e.g., transform a custom resolution to a tarball URL and use
   * fetchers.remoteTarball).
   *
   * @param cafs - The content-addressable file system to add package files to
   * @param resolution - The resolution object containing fetch information
   * @param opts - Fetch options including package manifest
   * @param fetchers - Standard pnpm fetchers available for delegation (remoteTarball, localTarball, git, etc.)
   * @returns FetchResult with files index and other package information
   */
  fetch?: (cafs: Cafs, resolution: Resolution, opts: FetchOptions, fetchers: Fetchers) => FetchResult | Promise<FetchResult>
}

export {
  getCustomResolverCacheKey,
  getCachedCanResolve,
  setCachedCanResolve,
  checkCustomResolverCanResolve,
} from './customResolverCache.js'
