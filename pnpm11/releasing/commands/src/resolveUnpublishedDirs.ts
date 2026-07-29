import type { ReleasePlan } from '@pnpm/releasing.versioning'
import pLimit from 'p-limit'

import { createVersionPublishedChecker, type PreviousChangelogOptions } from './publish/previousChangelog.js'

const DEFAULT_NETWORK_CONCURRENCY = 16

export type CheckVersionPublished = (pkgName: string, version: string) => Promise<boolean>

export type UnpublishedProbeOptions = PreviousChangelogOptions & {
  /** Overridable for tests; production probes the registry. */
  checkVersionPublished?: CheckVersionPublished
  networkConcurrency?: number
  /** Manifest name → published name, from `publishedNameByManifestName`. */
  publishedNames?: ReadonlyMap<string, string>
}

/**
 * The releases in `plan` whose current version the registry does not have — {@link assembleReleasePlan}'s `unpublishedDirs`.
 *
 * A release is keyed by its manifest name, so `publishedNames` translates it
 * for the probe; without that a renamed project reads as never published and
 * debuts at its manifest version on every release.
 */
export async function resolveUnpublishedDirs (plan: ReleasePlan, opts: UnpublishedProbeOptions): Promise<Set<string>> {
  const checkVersionPublished = opts.checkVersionPublished ?? createVersionPublishedChecker(opts)
  const limit = pLimit(opts.networkConcurrency ?? DEFAULT_NETWORK_CONCURRENCY)
  const probed = await Promise.all(
    plan.releases.map((release) => limit(async () => ({
      dir: release.dir,
      published: await checkVersionPublished(opts.publishedNames?.get(release.name) ?? release.name, release.currentVersion),
    })))
  )
  return new Set(probed.filter(({ published }) => !published).map(({ dir }) => dir))
}
