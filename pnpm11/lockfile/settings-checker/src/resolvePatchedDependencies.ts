import path from 'node:path'

export function resolvePatchedDependencies (
  patchedDependencies: Record<string, string> | undefined,
  baseDir: string
): Record<string, string> | undefined {
  if (!patchedDependencies) return undefined
  return Object.fromEntries(
    Object.entries(patchedDependencies).map(([key, value]) => [
      key,
      path.resolve(baseDir, value),
    ])
  )
}
