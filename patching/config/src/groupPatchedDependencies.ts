import * as dp from '@pnpm/dependency-path'
import { PnpmError } from '@pnpm/error'
import { type PatchFile, type PatchInfo } from '@pnpm/patching.types'
import { validRange } from 'semver'

export interface ExtendedPatchInfo extends PatchInfo {
  key: string
}

/** A group of {@link ExtendedPatchInfo}s which correspond to a package name. */
export interface PatchGroup {
  /** Maps exact versions to {@link ExtendedPatchInfo}. */
  exact: Record<string, ExtendedPatchInfo>
  /** Maps version ranges to {@link ExtendedPatchInfo}. */
  range: Record<string, ExtendedPatchInfo>
  /** The {@link ExtendedPatchInfo} without exact versions or version ranges. */
  blank: ExtendedPatchInfo | undefined
}

/** Maps package names to their corresponding groups. */
export type PatchGroupRecord = Record<string, PatchGroup>

export function groupPatchedDependencies (patchedDependencies: Record<string, PatchFile>): PatchGroupRecord {
  const result: PatchGroupRecord = {}
  function getGroup (name: string): PatchGroup {
    let group: PatchGroup | undefined = result[name]
    if (group) return group
    group = {
      exact: {},
      range: {},
      blank: undefined,
    }
    result[name] = group
    return group
  }

  for (const key in patchedDependencies) {
    const file = patchedDependencies[key]
    const { name, version, nonSemverVersion } = dp.parse(key)

    if (name && version) {
      getGroup(name).exact[version] = { strict: true, file, key }
      continue
    }

    if (name && nonSemverVersion) {
      if (!validRange(nonSemverVersion)) {
        throw new PnpmError('PATCH_NON_SEMVER_RANGE', `${nonSemverVersion} is not a valid semantic version range.`)
      }
      if (nonSemverVersion.trim() === '*') {
        getGroup(name).blank = { strict: true, file, key }
      } else {
        getGroup(name).range[nonSemverVersion] = { strict: true, file, key }
      }
      continue
    }

    // Set `strict` to `false` to preserve backward compatibility.
    getGroup(key).blank = { strict: false, file, key }
  }

  return result
}
