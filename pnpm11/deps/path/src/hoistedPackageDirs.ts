import path from 'node:path'

import { removeSuffix } from './index.js'

/**
 * The hoisted linker collapses the peer and patch variants of one package
 * version onto the first dependency path it meets, so `hoistedLocations`
 * records only that one. Key its locations by the path without the peer
 * and patch suffixes too, for the variants it left out.
 */
export function withCollapsedVariants (hoistedLocations: Record<string, string[]>): Record<string, string[]> {
  const result = { ...hoistedLocations }
  for (const [depPath, locations] of Object.entries(hoistedLocations)) {
    result[removeSuffix(depPath)] ??= locations
  }
  return result
}

/**
 * The directories that `hoistedLocations` records for `depPath`, resolved
 * against `lockfileDir`. A location that would leave `lockfileDir` is dropped.
 */
export function findHoistedPackageDirs (
  hoistedLocations: Record<string, string[]> | undefined,
  depPath: string,
  lockfileDir: string
): string[] {
  if (hoistedLocations == null) return []
  const keys = [depPath, removeSuffix(depPath)]
    .flatMap((key) => [key, key.startsWith('/') ? key.slice(1) : `/${key}`])
  const locations = keys.map((key) => hoistedLocations[key]).find((recorded) => recorded != null) ?? []
  return locations
    .map((location) => hoistedPackageDir(lockfileDir, location))
    .filter((dir): dir is string => dir != null)
}

/**
 * A lockfile-relative hoisted location resolved against `lockfileDir`, with
 * `/` and `\` both read as separators, or `undefined` for a location that
 * leaves it.
 */
function hoistedPackageDir (lockfileDir: string, location: string): string | undefined {
  if (path.isAbsolute(location) || location.startsWith('\\')) return undefined
  const dir = path.join(lockfileDir, ...location.split(/[/\\]/))
  const relative = path.relative(lockfileDir, dir)
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) return undefined
  return dir
}
