import type { Lockfile } from '@pnpm/lockfile-file'

export interface PreResolutionHookContext {
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  lockfileDir: string
  storeDir: string
}

export interface PreResolutionHookLogger {
  info: (message: string) => void
  warn: (message: string) => void
}

export type PreResolutioneHook = (ctx: PreResolutionHookContext, logger: PreResolutionHookLogger) => Promise<void>
