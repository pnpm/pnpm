import type { Lockfile } from '@pnpm/lockfile-file'
import { Logger } from '@pnpm/logger'

export interface PreResolveHookContext {
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  lockfileDir: string
  storeDir: string
  logger: Logger<unknown>
}

export type PreResolveHook = (ctx: PreResolveHookContext) => Promise<void>
