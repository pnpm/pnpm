import { type PatchFile, type PatchInfo } from '@pnpm/patching.types'

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
      file: patchedDependencies[pkgNameAndVersion],
      key: pkgNameAndVersion,
      strict: false,
    }
  }
  if (patchedDependencies[pkgName]) {
    return {
      file: patchedDependencies[pkgName],
      key: pkgName,
      strict: true,
    }
  }
  return undefined
}
