import * as dp from '@pnpm/dependency-path'
import { PnpmError } from '@pnpm/error'
import { type PatchFile, type PatchGroup, type PatchGroupRecord } from '@pnpm/patching.types'
import { validRange } from 'semver'

export function groupPatchedDependencies (patchedDependencies: Record<string, PatchFile>): PatchGroupRecord {
  const result: PatchGroupRecord = {}
  function getGroup (name: string): PatchGroup {
    let group: PatchGroup | undefined = result[name]
    if (group) return group
    group = {
      exact: {},
      range: [],
      all: undefined,
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
        getGroup(name).all = { strict: true, file, key }
      } else {
        getGroup(name).range.push({
          version: nonSemverVersion,
          patch: { strict: true, file, key },
        })
      }
      continue
    }

    // Set `strict` to `false` to preserve backward compatibility.
    getGroup(key).all = { strict: false, file, key }
  }

  return result
}
