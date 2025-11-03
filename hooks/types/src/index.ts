import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Registries, type BaseManifest } from '@pnpm/types'

// readPackage hook
export interface ReadPackageHookContext {
  log: (message: string) => void
}

export type ReadPackageHook = (pkg: BaseManifest, context: ReadPackageHookContext) => BaseManifest | Promise<BaseManifest>

// afterAllResolved hook
export interface AfterAllResolvedHookContext {
  log: (message: string) => void
}

export type AfterAllResolvedHook = (lockfile: LockfileObject, context: AfterAllResolvedHookContext) => LockfileObject | Promise<LockfileObject>

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

export interface PreResolutionHookResult {
  forceFullResolution?: boolean
}

export type PreResolutionHook = (ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) => Promise<PreResolutionHookResult | undefined>

// Custom resolver hooks
export type { PackageDescriptor, ResolveOptions, ResolveResult, ResolverPlugin } from '@pnpm/types'
