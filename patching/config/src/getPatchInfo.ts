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

  const satisfied = patchFileGroups[pkgName].range.filter(item => satisfies(pkgVersion, item.version))
  if (satisfied.length > 1) {
    const pkgId = `${pkgName}@${pkgVersion}`
    const message = `Unable to choose between ${satisfied.length} version ranges to patch ${pkgId}: ${satisfied.map(x => x.version).join(', ')}`
    throw new PnpmError('PATCH_KEY_CONFLICT', message, {
      hint: `Explicitly set the exact version (${pkgId}) to resolve conflict`,
    })
  }
  if (satisfied.length === 1) {
    return satisfied[0].patch
  }

  return patchFileGroups[pkgName].all
}
