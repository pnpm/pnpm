import fs from 'node:fs/promises'

import { PnpmError } from '@pnpm/error'
import type { VersioningSettings } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'

import type { ReleasePlan } from './assembleReleasePlan.js'
import { composeChangelogSection, prependChangelogSection } from './changelog.js'
import type { ChangeIntent } from './intents.js'
import { appendToLedger, getPackageConsumption, type Ledger, readLedger } from './ledger.js'

export interface ApplyReleasePlanOptions {
  workspaceDir: string
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

  const newEntries: Ledger = {}
  for (const release of plan.releases) {
    if (release.intents.length === 0) continue
    newEntries[`${release.name}@${release.newVersion}`] = release.intents.map((intent) => intent.id).sort()
  }
  await appendToLedger(opts.workspaceDir, newEntries)

  const ledger = await readLedger(opts.workspaceDir)
  await deleteConsumedIntentFiles(opts.allIntents, ledger, opts.versioning)

  return applied
}

/**
 * An intent file is deletable once every package it names has a ledger entry
 * for it, with one exemption: while a package is still on a prerelease line,
 * entries against prerelease versions alone keep the file alive — its prose
 * is still needed to compose the stable changelog section at graduation.
 * Declined (`none`) entries demand no release and never block deletion.
 */
async function deleteConsumedIntentFiles (allIntents: ChangeIntent[], ledger: Ledger, versioning?: VersioningSettings): Promise<void> {
  const prereleases = versioning?.prereleases ?? {}
  const deletable = allIntents.filter((intent) =>
    Object.entries(intent.releases).every(([pkgName, bumpType]) => {
      if (bumpType === 'none') return true
      const consumption = getPackageConsumption(ledger, pkgName)
      return consumption.allIds.has(intent.id) &&
        !(prereleases[pkgName] != null && consumption.prereleaseOnlyIds.has(intent.id))
    }))
  await Promise.all(deletable.map(async (intent) => fs.rm(intent.filePath)))
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
