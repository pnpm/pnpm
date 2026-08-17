import { globalInfo } from '@pnpm/logger'
import type { VerifiedFileIntegrity } from '@pnpm/store.cafs'

/**
 * Spending this long re-hashing store files is well past what a healthy
 * store needs, so the install owns up to the time.
 *
 * A time threshold rather than a file count: a thousand small files can
 * hash in a blink, while a handful of multi-megabyte blobs can stall an
 * install for seconds. The time is what the message claims, so the time
 * is what gates it.
 */
const REPORT_THRESHOLD_MS = 1000

/**
 * Tell the user when store verification spent a noticeable amount of
 * time re-hashing files, so a slow install has a visible cause.
 *
 * `verified` covers this install alone, and its `ms` is summed across
 * the workers that did the hashing, so it is the work spent and can
 * exceed the install's wall-clock time.
 *
 * The seconds are formatted with one decimal rather than through
 * `pretty-ms` because pacquet renders the same message from the same
 * figures, and the two have to agree character for character.
 */
export function reportVerifiedFileIntegrity (verified: VerifiedFileIntegrity): void {
  if (verified.ms <= REPORT_THRESHOLD_MS) return
  const seconds = (verified.ms / 1000).toFixed(1)
  globalInfo(`The integrity of ${verified.files} files was checked in ${seconds}s. This might have caused installation to take longer.`)
}
