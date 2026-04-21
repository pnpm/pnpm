import fs from 'fs'
import path from 'path'
import util from 'util'

/**
 * Walks up from `cwd` looking for a package.json with a `packageManager` field
 * and extracts the major version of pnpm from it.
 *
 * Returns `null` when no such file exists on the path, when the field is
 * malformed, or when the specified package manager isn't pnpm.
 *
 * This is called from `pnpm.ts` to decide whether to skip the legacy
 * argv[0]-driven npm passthrough for commands that pnpm v11 implements
 * natively — see pnpm/pnpm#11328.
 */
export function readWantedPnpmMajor (cwd: string = process.cwd()): number | null {
  let dir = cwd
  while (true) {
    const manifestPath = path.join(dir, 'package.json')
    let raw: string
    try {
      raw = fs.readFileSync(manifestPath, 'utf8')
    } catch (err: unknown) {
      // Only treat "manifest isn't here" as a reason to keep walking up.
      // Permission/IO errors should surface as "unknown" (null) instead of
      // silently consulting an ancestor manifest.
      if (!util.types.isNativeError(err) || !('code' in err)) return null
      if (err.code !== 'ENOENT' && err.code !== 'ENOTDIR') return null
      const parent = path.dirname(dir)
      if (parent === dir) return null
      dir = parent
      continue
    }
    let manifest: unknown
    try {
      manifest = JSON.parse(raw)
    } catch {
      return null
    }
    const pm = (manifest as { packageManager?: unknown }).packageManager
    if (typeof pm === 'string') {
      // packageManager is formatted as "<name>@<version>[+<integrity>]"
      const match = /^pnpm@(\d+)\./.exec(pm)
      return match ? parseInt(match[1], 10) : null
    }
    // In a workspace, leaf packages commonly omit packageManager while the
    // workspace root declares it. Keep walking up until we find a manifest
    // that does declare one (or we hit the filesystem root).
    const parent = path.dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}
