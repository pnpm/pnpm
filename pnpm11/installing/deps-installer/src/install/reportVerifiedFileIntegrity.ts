import { globalInfo } from '@pnpm/logger'
import type { VerifiedFileIntegrity } from '@pnpm/store.cafs'

/**
 * Spending this long re-hashing store files is well past what a healthy
 * store needs, so the install owns up to the time.
 */
const SLOW_MS = 1000

/**
 * Re-hashing this many files says something keeps invalidating the
 * store even when the hashing itself was quick — worth telling the user
 * about before the store grows and the same churn does cost them time.
 */
const MANY_FILES = 1000

/**
 * Tell the user when store verification re-hashed files: how much time
 * it cost, or failing that, that it happened at all on a scale a
 * healthy store never reaches. The two are separate claims, so they are
 * separate messages, and the timed one wins when both hold — it carries
 * the file count anyway.
 *
 * `verified` covers this install alone, and its `ms` is summed across
 * the workers that did the hashing, so it is the work spent and can
 * exceed the install's wall-clock time.
 *
 * The seconds are rounded to tenths in integer arithmetic rather than
 * by float formatting: pacquet renders the same messages from the same
 * figures and the two have to agree character for character, but
 * `toFixed` rounds a tie up where Rust's `{:.1}` rounds it to even.
 */
export function reportVerifiedFileIntegrity (verified: VerifiedFileIntegrity): void {
  if (verified.ms > SLOW_MS) {
    const tenths = Math.floor((verified.ms + 50) / 100)
    globalInfo(`The integrity of ${verified.files} files was checked in ${Math.floor(tenths / 10)}.${tenths % 10}s.`)
  } else if (verified.files > MANY_FILES) {
    globalInfo(`The integrity of ${verified.files} files was checked, because their timestamps changed since the store recorded them. A backup tool, an antivirus scan, or a copied store can cause this.`)
  }
}
