import { type PatchFile, type PatchInfo } from '@pnpm/types'

export function getPatchInfo (
  patchedDependencies: Record<string, PatchFile> | undefined,
  pkgName: string,
  pkgVersion: string
): PatchInfo | undefined {
  if (!patchedDependencies) return undefined
  const pkgNameAndVersion = `${pkgName}@${pkgVersion}`
  if (patchedDependencies[pkgNameAndVersion]) {
    return {
      appliedToAnyVersion: false,
      file: patchedDependencies[pkgNameAndVersion],
    }
  }
  if (patchedDependencies[pkgName]) {
    return {
      appliedToAnyVersion: true,
      file: patchedDependencies[pkgName],
    }
  }
  return undefined
}
