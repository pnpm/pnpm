import { DEFAULT_OPTS } from '@pnpm/testing.command-defaults'

export { DEFAULT_OPTS }

const IS_ODD_0_1_1_PUBLISHED_AT = Date.parse('2016-12-07T07:18:01.205Z')

/**
 * A `minimumReleaseAge` whose cutoff falls a minute before is-odd@0.1.1 was
 * published. The margin keeps the cutoff before the publish time even though
 * the install computes it from a later `Date.now()`.
 */
export function minimumReleaseAgeRejectingIsOdd011 (): number {
  return Math.ceil((Date.now() - IS_ODD_0_1_1_PUBLISHED_AT) / 60_000) + 1
}
