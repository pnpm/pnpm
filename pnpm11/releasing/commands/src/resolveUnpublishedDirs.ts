import type { ReleasePlan } from '@pnpm/releasing.versioning'

import { createVersionPublishedChecker, type PreviousChangelogOptions } from './publish/previousChangelog.js'

/**
 * Resolves whether `pkgName@version` is already published — the seam that
 * decides whether a release is a package's first (publish the version
 * verbatim) or a follow-up (bump). Production leaves it unset and the registry
 * is probed; tests override it to steer the outcome without a network round
 * trip, matching the injected `verifyPublished` gate the apply path uses.
 */
export type CheckVersionPublished = (pkgName: string, version: string) => Promise<boolean>

export type UnpublishedProbeOptions = PreviousChangelogOptions & {
  checkVersionPublished?: CheckVersionPublished
}

/**
 * The directories in `plan` whose current manifest version is not yet on the
 * registry — the packages whose first release must publish that version
 * verbatim instead of bumping off it. Probes every release concurrently; a
 * probe failure rejects, so the surrounding command fails rather than release
 * a wrong version. Feeds {@link assembleReleasePlan}'s `unpublishedDirs`.
 *
 * A first assembly pass (without `unpublishedDirs`) supplies `plan`. Holding a
 * package at its current version can only remove dependent propagation, never
 * add it (the materialized range already admits the unchanged version), so the
 * re-assembled plan is a subset of `plan` — every dir it can hold was probed
 * here.
 */
export async function resolveUnpublishedDirs (plan: ReleasePlan, opts: UnpublishedProbeOptions): Promise<Set<string>> {
  const checkVersionPublished = opts.checkVersionPublished ?? createVersionPublishedChecker(opts)
  const probed = await Promise.all(
    plan.releases.map(async (release) => ({
      dir: release.dir,
      published: await checkVersionPublished(release.name, release.currentVersion),
    }))
  )
  return new Set(probed.filter(({ published }) => !published).map(({ dir }) => dir))
}
