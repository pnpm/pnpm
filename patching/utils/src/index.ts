import { type PatchFile, type PatchInfo } from '@pnpm/types'

export interface ExtendedPatchInfo extends PatchInfo {
  key: string
}

export function getPatchInfo (
  patchedDependencies: Record<string, PatchFile> | undefined,
  pkgName: string,
  pkgVersion: string
): ExtendedPatchInfo | undefined {
  if (!patchedDependencies) return undefined
  const pkgNameAndVersion = `${pkgName}@${pkgVersion}`
  if (patchedDependencies[pkgNameAndVersion]) {
    return {
      appliedToAnyVersion: false,
      file: patchedDependencies[pkgNameAndVersion],
      key: pkgNameAndVersion,
    }
  }
  if (patchedDependencies[pkgName]) {
    return {
      appliedToAnyVersion: true,
      file: patchedDependencies[pkgName],
      key: pkgName,
    }
  }
  return undefined
}
