import fs from 'fs'
import path from 'path'

/**
 * Walks up from `cwd` looking for the nearest package.json and extracts the
 * major version of pnpm from its `packageManager` field.
 *
 * Returns `null` when no package.json exists, when the field is absent or
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
    } catch {
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
    if (typeof pm !== 'string') return null
    // packageManager is formatted as "<name>@<version>[+<integrity>]"
    const match = /^pnpm@(\d+)\./.exec(pm)
    return match ? parseInt(match[1], 10) : null
  }
}
