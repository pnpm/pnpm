import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { readPackageJsonFromDirRawSync, safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { PackageManifest } from '@pnpm/types'

const RESERVED_ALIASES = new Set(['node_modules', 'favicon.ico'])

const isUrlFriendly = (segment: string): boolean => encodeURIComponent(segment) === segment

// A dependency alias from a global group's package.json becomes a directory
// name under `node_modules` at every join site (list, conflict-check,
// remove, update). A tampered manifest could use an alias like `../x` or an
// absolute path to escape the install dir, so only valid npm package names
// are trusted. This applies the same `validForOldPackages` rules that
// `validate-npm-package-name` (and `@pnpm/fs.symlink-dependency`'s
// `safeJoinModulesDir`) enforce — implemented inline to avoid adding a
// dependency to this low-level package.
export function isValidGlobalDependencyAlias (alias: string): boolean {
  if (alias.length === 0) return false
  if (/^[._-]/.test(alias)) return false
  if (alias.trim() !== alias) return false
  if (RESERVED_ALIASES.has(alias.toLowerCase())) return false
  if (isUrlFriendly(alias)) return true
  const scoped = /^@([^/]+)\/([^/]+)$/.exec(alias)
  if (scoped) {
    const [, scope, name] = scoped
    return !name.startsWith('.') && isUrlFriendly(scope) && isUrlFriendly(name)
  }
  return false
}

function pickValidDependencies (dependencies: Record<string, string>): Record<string, string> {
  const result: Record<string, string> = {}
  for (const [alias, spec] of Object.entries(dependencies)) {
    if (isValidGlobalDependencyAlias(alias)) {
      result[alias] = spec
    }
  }
  return result
}

export interface GlobalPackageInfo {
  hash: string
  installDir: string
  dependencies: Record<string, string>
}

export interface InstalledGlobalPackage {
  alias: string
  version: string
  manifest: PackageManifest
}

export function scanGlobalPackages (globalDir: string): GlobalPackageInfo[] {
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(globalDir, { withFileTypes: true })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  const result: GlobalPackageInfo[] = []
  for (const entry of entries) {
    // Hash entries are symlinks pointing to install dirs
    if (!entry.isSymbolicLink()) continue
    const linkPath = path.join(globalDir, entry.name)
    let installDir: string
    try {
      installDir = fs.realpathSync(linkPath)
    } catch {
      continue
    }
    let pkgJson: PackageManifest
    try {
      pkgJson = readPackageJsonFromDirRawSync(installDir)
    } catch {
      continue
    }
    if (!pkgJson.dependencies) continue
    const dependencies = pickValidDependencies(pkgJson.dependencies)
    if (Object.keys(dependencies).length === 0) continue
    result.push({
      hash: entry.name,
      installDir,
      dependencies,
    })
  }
  return result
}

export function findGlobalPackage (globalDir: string, alias: string): GlobalPackageInfo | null {
  const packages = scanGlobalPackages(globalDir)
  return packages.find((pkg) => alias in pkg.dependencies) ?? null
}

export async function getGlobalPackageDetails (info: GlobalPackageInfo): Promise<InstalledGlobalPackage[]> {
  const aliases = Object.keys(info.dependencies)
  const installedPackages = await Promise.all(
    aliases.map(async (alias): Promise<InstalledGlobalPackage | null> => {
      const manifest = await safeReadPackageJsonFromDir(path.join(info.installDir, 'node_modules', alias))
      if (!manifest) return null
      return { alias, version: manifest.version, manifest }
    })
  )
  return installedPackages.filter((pkg): pkg is InstalledGlobalPackage => pkg !== null)
}

export function cleanOrphanedInstallDirs (globalDir: string): void {
  globalDir = path.resolve(globalDir)
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(globalDir, { withFileTypes: true })
  } catch {
    return
  }

  // Collect real paths of all symlink targets
  const referenced = new Set<string>()
  for (const entry of entries) {
    if (!entry.isSymbolicLink()) continue
    try {
      referenced.add(fs.realpathSync(path.join(globalDir, entry.name)))
    } catch {}
  }

  // Remove directories that no symlink points to.
  // Skip recently-created dirs to avoid racing with a concurrent install
  // that hasn't created its hash symlink yet.
  const now = Date.now()
  const SAFETY_WINDOW_MS = 5 * 60 * 1000
  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    const dirPath = path.join(globalDir, entry.name)
    if (referenced.has(dirPath)) continue
    try {
      const stat = fs.statSync(dirPath)
      if (now - Math.max(stat.birthtimeMs, stat.ctimeMs) < SAFETY_WINDOW_MS) continue
    } catch {
      continue
    }
    fs.rmSync(dirPath, { recursive: true, force: true })
  }
}

export async function getInstalledBinNames (info: GlobalPackageInfo): Promise<string[]> {
  const bins = new Set<string>()
  const aliases = Object.keys(info.dependencies)
  const modulesDir = path.join(info.installDir, 'node_modules')
  await Promise.all(
    aliases.map(async (alias) => {
      const depDir = path.join(modulesDir, alias)
      const manifest = await safeReadPackageJsonFromDir(depDir)
      if (!manifest) return
      const binsOfPkg = await getBinsFromPackageManifest(manifest, depDir)
      for (const bin of binsOfPkg) {
        bins.add(bin.name)
      }
    })
  )
  return [...bins]
}
