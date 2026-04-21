import fs from 'fs'
import path from 'path'
import util from 'util'

/**
 * Walks up from `cwd` looking for a package.json that declares a
 * `packageManager` field and, when that package manager is pnpm, returns its
 * major version.
 *
 * Returns `null` when no such manifest exists on the path, when a manifest is
 * malformed (unreadable for a reason other than "not found", invalid JSON,
 * non-object root, or a non-string `packageManager`), or when the specified
 * package manager isn't pnpm.
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
    if (manifest === null || typeof manifest !== 'object' || Array.isArray(manifest)) {
      return null
    }
    const obj = manifest as Record<string, unknown>
    if (!Object.prototype.hasOwnProperty.call(obj, 'packageManager')) {
      // In a workspace, leaf packages commonly omit packageManager while the
      // workspace root declares it. Keep walking up until we find a manifest
      // that does declare one (or we hit the filesystem root).
      const parent = path.dirname(dir)
      if (parent === dir) return null
      dir = parent
      continue
    }
    const pm = obj.packageManager
    if (typeof pm !== 'string') return null
    // packageManager is formatted as "<name>@<version>[+<integrity>]"
    const match = /^pnpm@(\d+)\./.exec(pm)
    return match ? parseInt(match[1], 10) : null
  }
}

export interface PassthroughEnv {
  COREPACK_ROOT?: string
  npm_config_manage_package_manager_versions?: string
}

/**
 * Decides whether `pnpm.ts`'s argv[0] switch should route a passthrough-prefix
 * command (`version`, `docs`, …) into `main()` instead of forwarding it to
 * npm.
 *
 * We only skip the legacy passthrough when all of the following hold:
 *   - Corepack isn't driving pnpm (it already picked the binary).
 *   - Version switching isn't disabled via env or a project `.npmrc`. If it
 *     is, `main()` won't switch to the wanted pnpm, so the only way the
 *     command resolves to anything sensible is via the npm passthrough.
 *   - The project's `packageManager` selects pnpm v11 or newer, which is the
 *     first line of pnpm that implements these commands natively.
 */
export function shouldSkipNpmPassthrough (env: PassthroughEnv, cwd: string = process.cwd()): boolean {
  if (env.COREPACK_ROOT != null) return false
  if (env.npm_config_manage_package_manager_versions === 'false') return false
  if (readManagePackageManagerVersionsSetting(cwd) === false) return false
  const wantedMajor = readWantedPnpmMajor(cwd)
  return wantedMajor != null && wantedMajor >= 11
}

/**
 * Walks up from `cwd` looking for the first `.npmrc` that explicitly sets
 * `manage-package-manager-versions`. Returns the effective value, or `null`
 * when none of the `.npmrc` files on the path sets it (callers should then
 * treat the setting as its default of `true`).
 *
 * This is a deliberately narrow reader — pnpm's real config system is far
 * richer, but pulling it in at CLI entry would defeat the point of keeping
 * cold start cheap. The walk-up matches how npm/pnpm locate the project
 * `.npmrc`, which is the dominant place users set this flag.
 */
export function readManagePackageManagerVersionsSetting (cwd: string = process.cwd()): boolean | null {
  const regex = /^[ \t]*manage-package-manager-versions[ \t]*=[ \t]*(true|false)[ \t]*(?:[;#].*)?$/im
  let dir = cwd
  while (true) {
    const npmrcPath = path.join(dir, '.npmrc')
    let raw: string | null = null
    try {
      raw = fs.readFileSync(npmrcPath, 'utf8')
    } catch (err: unknown) {
      if (!util.types.isNativeError(err) || !('code' in err)) return null
      if (err.code !== 'ENOENT' && err.code !== 'ENOTDIR') return null
    }
    if (raw != null) {
      const match = regex.exec(raw)
      if (match) return match[1].toLowerCase() === 'true'
    }
    const parent = path.dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}
