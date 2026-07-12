import fs from 'node:fs/promises'

import { PnpmError } from '@pnpm/error'
import type { VersioningSettings } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'

import { indexProjectRefs, type ReleasePlan, type WorkspaceProject } from './assembleReleasePlan.js'
import { composeChangelogSection, prependChangelogSection } from './changelog.js'
import type { ChangeIntent } from './intents.js'
import { appendToLedger, buildConsumptionIndex } from './ledger.js'

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
   * Snapshot releases only rewrite manifest versions: they consume no intent
   * files, write no changelogs, and leave the ledger untouched.
   */
  snapshot?: boolean
}

export interface AppliedRelease {
  name: string
  currentVersion: string
  newVersion: string
}

export async function applyReleasePlan (plan: ReleasePlan, opts: ApplyReleasePlanOptions): Promise<AppliedRelease[]> {
  assertSupportedChangelogStorage(opts.versioning)

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

  if (opts.snapshot) {
    return applied
  }

  await Promise.all(plan.releases.map(async (release) => {
    const section = composeChangelogSection(release)
    await prependChangelogSection(release.rootDir, release.name, section)
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

  // An intent file is deletable once every project it names has a ledger
  // entry for it, with one exemption: while a project is still on a lane,
  // entries against prerelease versions alone keep the file alive — its
  // prose is still needed to compose the stable changelog section at
  // graduation. Declined (`none`) entries demand no release and never block
  // deletion. References here were already validated by the plan assembly,
  // so an unresolvable one just keeps its file around.
  const refs = indexProjectRefs(opts.projects, opts.workspaceDir)
  const consumptionOf = buildConsumptionIndex(ledger, refs.nameToDirs)
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

function assertSupportedChangelogStorage (versioning?: VersioningSettings): void {
  const storage = versioning?.changelog?.storage
  if (storage != null && storage !== 'repository') {
    throw new PnpmError(
      'VERSIONING_UNSUPPORTED_CHANGELOG_STORAGE',
      `versioning.changelog.storage "${storage}" is not implemented yet. Use "repository".`
    )
  }
}
