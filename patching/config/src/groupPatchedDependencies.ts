import * as dp from '@pnpm/dependency-path'
import { PnpmError } from '@pnpm/error'
import type { PatchGroup, PatchGroupRecord } from '@pnpm/patching.types'
import { validRange } from 'semver'

export function groupPatchedDependencies (patchedDependencies: Record<string, string>): PatchGroupRecord {
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
    const hash = patchedDependencies[key]
    const { name, version, nonSemverVersion } = dp.parse(key)

    if (name && version) {
      getGroup(name).exact[version] = { hash, key }
      continue
    }

    if (name && nonSemverVersion) {
      if (!validRange(nonSemverVersion)) {
        throw new PnpmError('PATCH_NON_SEMVER_RANGE', `${nonSemverVersion} is not a valid semantic version range.`)
      }
      if (nonSemverVersion.trim() === '*') {
        getGroup(name).all = { hash, key }
      } else {
        getGroup(name).range.push({
          version: nonSemverVersion,
          patch: { hash, key },
        })
      }
      continue
    }

    getGroup(key).all = { hash, key }
  }

  return result
}
