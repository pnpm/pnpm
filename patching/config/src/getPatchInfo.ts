import { PnpmError } from '@pnpm/error'
import { type ExtendedPatchInfo, type PatchGroupRecord } from '@pnpm/patching.types'
import { satisfies } from 'semver'

export function getPatchInfo (
  patchFileGroups: PatchGroupRecord | undefined,
  pkgName: string,
  pkgVersion: string
): ExtendedPatchInfo | undefined {
  if (!patchFileGroups?.[pkgName]) return undefined

  const exactVersion = patchFileGroups[pkgName].exact[pkgVersion]
  if (exactVersion) return exactVersion

  const versionRanges = Object
    .keys(patchFileGroups[pkgName].range)
    .filter(range => satisfies(pkgVersion, range))
  if (versionRanges.length > 1) {
    const pkgId = `${pkgName}@${pkgVersion}`
    const message = `Unable to choose between ${versionRanges.length} version ranges to patch ${pkgId}: ${versionRanges.join(', ')}`
    throw new PnpmError('PATCH_KEY_CONFLICT', message, {
      hint: `Explicitly set the exact version (${pkgId}) to resolve conflict`,
    })
  }
  if (versionRanges.length === 1) {
    return patchFileGroups[pkgName].range[versionRanges[0]]
  }

  return patchFileGroups[pkgName].all
}
