import fs from 'node:fs/promises'

import type { VersioningChangelogStorage, VersioningSettings } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'

import { indexProjectRefs, type ReleasePlan, type WorkspaceProject } from './assembleReleasePlan.js'
import { composeChangelogSection, prependChangelogSection } from './changelog.js'
import type { ChangeIntent } from './intents.js'
import { appendToLedger, buildConsumptionIndex, type Ledger, ledgerEntryIds } from './ledger.js'
import { readPendingChangelog, removePendingChangelog, writePendingChangelog } from './pendingChangelog.js'

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
    : await confirmPublishedLedger(ledger, opts)
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
 * Narrows `ledger` to the entries the registry confirms are published with
 * their parked changelog section, deleting each confirmed section file (its
 * prose now lives in the published tarball). Only entries still referenced by
 * an unconsumed intent are checked, so already-collected historical releases
 * — which no longer have a parked section — cost no network round-trips.
 */
async function confirmPublishedLedger (ledger: Ledger, opts: ApplyReleasePlanOptions): Promise<Ledger> {
  const { verifyPublished } = opts
  const confirmed: Ledger = Object.create(null) as Ledger
  if (verifyPublished == null) return confirmed
  const pendingIds = new Set(opts.allIntents.map((intent) => intent.id))
  await Promise.all(Object.entries(ledger).map(async ([key, entry]) => {
    if (!ledgerEntryIds(entry).some((id) => pendingIds.has(id))) return
    const atIndex = key.lastIndexOf('@')
    if (atIndex <= 0) return
    const name = key.slice(0, atIndex)
    const version = key.slice(atIndex + 1)
    const section = await readPendingChangelog(opts.workspaceDir, name, version)
    if (section == null) return
    if (await verifyPublished(name, version, section)) {
      confirmed[key] = entry
      await removePendingChangelog(opts.workspaceDir, name, version)
    }
  }))
  return confirmed
}
