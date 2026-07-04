import path from 'node:path'

import { map as mapValues } from 'ramda'

export function resolvePatchedDependencies (
  patchedDependencies: Record<string, string> | undefined,
  baseDir: string
): Record<string, string> | undefined {
  if (!patchedDependencies) return undefined
  return mapValues((patchFile) => path.resolve(baseDir, patchFile), patchedDependencies)
}
