import path from 'node:path'

import { removeSuffix } from './index.js'

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
  const legacyDepPath = depPath.startsWith('/') ? depPath.slice(1) : `/${depPath}`
  const locations = hoistedLocations[depPath] ??
    hoistedLocations[removeSuffix(depPath)] ??
    hoistedLocations[legacyDepPath] ??
    []
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
