import fs from 'node:fs/promises'

import type { VersioningChangelogStorage, VersioningSettings } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'

import { indexProjectRefs, type ReleasePlan, type WorkspaceProject } from './assembleReleasePlan.js'
import { composeChangelogSection, prependChangelogSection } from './changelog.js'
import type { ChangeIntent } from './intents.js'
import { appendToLedger, buildConsumptionIndex, type Ledger } from './ledger.js'
import { listPendingChangelogs, readPendingChangelog, removePendingChangelog, writePendingChangelog } from './pendingChangelog.js'

/**
 * Registry storage is the default: no CHANGELOG.md is committed; a release's
 * section is parked (see {@link writePendingChangelog}) and packed into the
 * published tarball. `repository` opts back into committed changelogs.
 */
export function changelogStorage (versioning?: VersioningSettings): VersioningChangelogStorage {
  return versioning?.changelog?.storage ?? 'registry'
}

export interface ApplyReleasePlanOptions {
  workspaceDir: string
  projects: WorkspaceProject[]
  /**
   * All intent files currently in the workspace, used to decide which files
   * are fully consumed after this run and can be deleted.
   */
  allIntents: ChangeIntent[]
  versioning?: VersioningSettings
  /**
   * In `registry` changelog storage, the gate that lets a ledgered release's
   * intents be garbage-collected: it must resolve `true` only once the
   * registry has published `${name}@${version}` and that tarball's
   * CHANGELOG.md already contains `section`. Without it, registry-mode
   * releases keep their intents (the repository is still their only prose).
   * Ignored in `repository` storage, where the committed changelog already
   * holds the prose and the ledger alone gates deletion.
   */
  verifyPublished?: (name: string, version: string, section: string) => Promise<boolean>
}

export interface AppliedRelease {
  name: string
  currentVersion: string
  newVersion: string
}

export async function applyReleasePlan (plan: ReleasePlan, opts: ApplyReleasePlanOptions): Promise<AppliedRelease[]> {
  const storage = changelogStorage(opts.versioning)

  const applied = await Promise.all(plan.releases.map(async (release) => {
    const { manifest, writeProjectManifest } = await readProjectManifest(release.rootDir)
    manifest.version = release.newVersion
    await writeProjectManifest(manifest)
    return {
      name: release.name,
      currentVersion: release.currentVersion,
      newVersion: release.newVersion,
    }
  }))

  // In `repository` storage the section is committed to CHANGELOG.md now. In
  // `registry` storage nothing is committed to the package; the section is
  // parked until publish, when it is packed into the tarball.
  await Promise.all(plan.releases.map(async (release) => {
    const section = composeChangelogSection(release)
    if (storage === 'repository') {
      await prependChangelogSection(release.rootDir, release.name, section)
    } else {
      await writePendingChangelog(opts.workspaceDir, release.name, release.newVersion, section)
    }
  }))

  const newEntries: Record<string, { dir: string, intents: string[] }> = {}
  for (const release of plan.releases) {
    if (release.intents.length === 0) continue
    newEntries[`${release.name}@${release.newVersion}`] = {
      dir: release.dir,
      intents: release.intents.map((intent) => intent.id).sort(),
    }
  }
  const ledger = await appendToLedger(opts.workspaceDir, newEntries)

  // An intent file is deletable once every project it names has a consumed
  // ledger entry for it, with one exemption: while a project is still on a
  // lane, entries against prerelease versions alone keep the file alive — its
  // prose is still needed to compose the stable changelog section at
  // graduation. Declined (`none`) entries demand no release and never block
  // deletion. References here were already validated by the plan assembly,
  // so an unresolvable one just keeps its file around.
  //
  // In `registry` storage the ledger alone does not authorize deletion: the
  // repository is the only copy of the prose until the release is published,
  // so an entry counts as consumed only once the registry confirms its version
  // carries the composed section.
  const refs = indexProjectRefs(opts.projects, opts.workspaceDir)
  const consumedLedger = storage === 'repository'
    ? ledger
    : await confirmPublished(ledger, opts)
  const consumptionOf = buildConsumptionIndex(consumedLedger, refs.nameToDirs)
  const laneDirs = new Set<string>()
  for (const ref of Object.keys(opts.versioning?.lanes ?? {})) {
    for (const dir of refs.refToDirs(ref)) {
      laneDirs.add(dir)
    }
  }
  const deletable = opts.allIntents.filter((intent) =>
    Object.entries(intent.releases).every(([ref, bumpType]) => {
      if (bumpType === 'none') return true
      const dirs = refs.refToDirs(ref)
      if (dirs.length !== 1) return false
      const consumption = consumptionOf(dirs[0])
      return consumption.allIds.has(intent.id) &&
        !(laneDirs.has(dirs[0]) && consumption.prereleaseOnlyIds.has(intent.id))
    }))
  await Promise.all(deletable.map(async (intent) => fs.rm(intent.filePath)))

  return applied
}

/**
 * Confirms the parked sections whose releases the registry reports published
 * with that section, deletes those section files (their prose now lives in the
 * published tarball), and returns the subset of `ledger` those confirmations
 * cover — the consumed ledger that gates intent deletion. Driven by the parked
 * files rather than the ledger, so a dependency-propagated release (which has a
 * section but no consumed intents, hence no ledger entry) still gets its
 * section collected. Every parked file belongs to an as-yet-unpublished
 * release, so the confirmation cost is bounded by the release backlog, not by
 * history.
 */
async function confirmPublished (ledger: Ledger, opts: ApplyReleasePlanOptions): Promise<Ledger> {
  const { verifyPublished } = opts
  const confirmed: Ledger = Object.create(null) as Ledger
  if (verifyPublished == null) return confirmed
  const pending = await listPendingChangelogs(opts.workspaceDir)
  await Promise.all(pending.map(async ({ name, version }) => {
    const section = await readPendingChangelog(opts.workspaceDir, name, version)
    if (section == null) return
    if (!(await verifyPublished(name, version, section))) return
    const key = `${name}@${version}`
    if (ledger[key] != null) confirmed[key] = ledger[key]
    await removePendingChangelog(opts.workspaceDir, name, version)
  }))
  return confirmed
}
