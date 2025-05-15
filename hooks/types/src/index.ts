import { type LockfileObject } from '@pnpm/lockfile.types'
import { type Registries } from '@pnpm/types'

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
