import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PatchGroup, PatchGroupRecord, PatchInfo } from '@pnpm/patching.types'
import { validRange } from 'semver'

export function groupPatchedDependencies (patchedDependencies: Record<string, string | PatchInfo>): PatchGroupRecord {
  // Keys come from `patchedDependencies`, so one named after an `Object.prototype` member must not
  // resolve to that member.
  const result: PatchGroupRecord = Object.create(null)
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
    const value = patchedDependencies[key]
    const info = typeof value === 'string' ? { hash: value } : value
    const { name, version, nonSemverVersion } = dp.parse(key)

    if (name && version) {
      getGroup(name).exact[version] = { ...info, key }
      continue
    }

    if (name && nonSemverVersion) {
      if (!validRange(nonSemverVersion)) {
        throw new PnpmError('PATCH_NON_SEMVER_RANGE', `${nonSemverVersion} is not a valid semantic version range.`)
      }
      if (nonSemverVersion.trim() === '*') {
        getGroup(name).all = { ...info, key }
      } else {
        getGroup(name).range.push({
          version: nonSemverVersion,
          patch: { ...info, key },
        })
      }
      continue
    }

    getGroup(key).all = { ...info, key }
  }

  return result
}

export function groupPatchedDependenciesWithPaths (
  patchedDependencies: Record<string, string> | undefined,
  resolvedPatchedDependencies: Record<string, string> | undefined
): PatchGroupRecord | undefined {
  if (!patchedDependencies) return undefined
  if (!resolvedPatchedDependencies) return groupPatchedDependencies(patchedDependencies)
  return groupPatchedDependencies(Object.fromEntries(
    Object.entries(patchedDependencies).map(([key, hash]) => {
      let patchFilePath: string | undefined = resolvedPatchedDependencies[key]
      if (!patchFilePath) {
        const lastAt = key.lastIndexOf('@')
        const pkgName = lastAt > 0 ? key.slice(0, lastAt) : key
        patchFilePath = resolvedPatchedDependencies[pkgName]
      }
      return [key, { hash, patchFilePath }]
    })
  ))
}
