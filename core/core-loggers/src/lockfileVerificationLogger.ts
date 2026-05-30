import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const lockfileVerificationLogger = logger<LockfileVerificationMessage>('lockfile-verification')

export interface LockfileVerificationMessageBase {
  status: 'started' | 'done' | 'failed'
  /**
   * Absolute path of the lockfile being verified. Omitted only when
   * the verifier is invoked without a path (today only in unit tests
   * that skip the cache wiring); production code paths always pass it.
   */
  lockfilePath?: string
}

export interface LockfileVerificationStartedMessage extends LockfileVerificationMessageBase {
  status: 'started'
  /** Number of distinct (name, version, resolution) entries about to be verified. */
  entries: number
}

export interface LockfileVerificationDoneMessage extends LockfileVerificationMessageBase {
  status: 'done'
  /** Number of distinct (name, version, resolution) entries that were verified. */
  entries: number
  /** Milliseconds elapsed between the matching `started` event and `done`. */
  elapsedMs: number
}

/**
 * Terminal event emitted on every exit path that emitted `started` but
 * didn't succeed — both policy violations (a `PnpmError` is about to be
 * thrown with the breakdown) and unexpected throws from the registry
 * fan-out. Lets the reporter close out the transient `started` frame
 * in ansi-diff mode so it isn't left looking like a hung spinner above
 * the failure output.
 */
export interface LockfileVerificationFailedMessage extends LockfileVerificationMessageBase {
  status: 'failed'
  /** Number of distinct (name, version, resolution) entries that were checked before the failure. */
  entries: number
  /** Milliseconds elapsed between the matching `started` event and `failed`. */
  elapsedMs: number
}

export type LockfileVerificationMessage =
  | LockfileVerificationStartedMessage
  | LockfileVerificationDoneMessage
  | LockfileVerificationFailedMessage

export type LockfileVerificationLog =
  & { name: 'pnpm:lockfile-verification' }
  & LogBase
  & LockfileVerificationMessage
