import * as dp from '@pnpm/dependency-path'
import { PnpmError } from '@pnpm/error'
import { type PatchFile } from '@pnpm/patching.types'
import { validRange } from 'semver'

/** A group of {@link PatchFile}s which correspond to a package name. */
export interface PatchFileGroup {
  /** Maps exact versions to {@link PatchFile}. */
  exact: Record<string, PatchFile>
  /** Maps version ranges to {@link PatchFile}. */
  range: Record<string, PatchFile>
  /** The {@link PatchFile} without exact versions or version ranges. */
  blank: PatchFile | undefined
}

/** Maps package names to their corresponding groups. */
export type PatchFileGroupRecord = Record<string, PatchFileGroup>

export function groupPatchedDependencies (patchedDependencies: Record<string, PatchFile>): PatchFileGroupRecord {
  const result: PatchFileGroupRecord = {}
  function getGroup (name: string): PatchFileGroup {
    let group: PatchFileGroup | undefined = result[name]
    if (group) return group
    group = {
      exact: {},
      range: {},
      blank: undefined,
    }
    result[name] = group
    return group
  }

  for (const patchKey in patchedDependencies) {
    const patchFile = patchedDependencies[patchKey]
    const { name, version, nonSemverVersion } = dp.parse(patchKey)

    if (name && version) {
      getGroup(name).exact[version] = patchFile
      continue
    }

    if (name && nonSemverVersion) {
      if (!validRange(nonSemverVersion)) {
        throw new PnpmError('PATCH_NON_SEMVER_RANGE', `${nonSemverVersion} is not a valid semantic version range.`)
      }
      if (nonSemverVersion.trim() === '*') {
        getGroup(name).blank = patchFile
      } else {
        getGroup(name).range[nonSemverVersion] = patchFile
      }
      continue
    }

    getGroup(patchKey).blank = patchFile
  }

  return result
}
