import { PnpmError } from '@pnpm/error'
import { type ExtendedPatchInfo, type PatchGroupRangeItem, type PatchGroupRecord } from '@pnpm/patching.types'
import { satisfies } from 'semver'

class PatchKeyConflictError extends PnpmError {
  constructor (
    pkgName: string,
    pkgVersion: string,
    satisfied: Array<Pick<PatchGroupRangeItem, 'version'>>
  ) {
    const pkgId = `${pkgName}@${pkgVersion}`
    const satisfiedVersions = satisfied.map(({ version }) => version)
    const message = `Unable to choose between ${satisfied.length} version ranges to patch ${pkgId}: ${satisfiedVersions.join(', ')}`
    super('PATCH_KEY_CONFLICT', message, {
      hint: `Explicitly set the exact version (${pkgId}) to resolve conflict`,
    })
  }
}

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
    throw new PatchKeyConflictError(pkgName, pkgVersion, satisfied)
  }
  if (satisfied.length === 1) {
    return satisfied[0].patch
  }

  return patchFileGroups[pkgName].all
}
