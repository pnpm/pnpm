import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const lockfileVerificationLogger = logger<LockfileVerificationMessage>('lockfile-verification')

export interface LockfileVerificationMessageBase {
  status: 'started' | 'done'
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

export type LockfileVerificationMessage =
  | LockfileVerificationStartedMessage
  | LockfileVerificationDoneMessage

export type LockfileVerificationLog =
  & { name: 'pnpm:lockfile-verification' }
  & LogBase
  & LockfileVerificationMessage
