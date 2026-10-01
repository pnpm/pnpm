import fs from 'node:fs'
import path from 'node:path'

import { type Command, getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { isError } from '@pnpm/error'
import { readPackageJsonFromDir, readPackageJsonFromDirRawSync, safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
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

export interface GlobalPackageBinSnapshot {
  info: GlobalPackageInfo
  binNames: string[]
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
    if (isNotFound(err)) return []
    throw err
  }
  const result: GlobalPackageInfo[] = []
  for (const entry of entries) {
    if (!entry.isSymbolicLink()) continue
    const info = scanSymlinkEntry(globalDir, entry.name)
    if (info) result.push(info)
  }
  return result
}

function scanSymlinkEntry (globalDir: string, entryName: string): GlobalPackageInfo | undefined {
  const linkPath = path.join(globalDir, entryName)
  try {
    const installDir = fs.realpathSync(linkPath)
    const pkgJson = readPackageJsonFromDirRawSync(installDir)
    if (!pkgJson.dependencies) return undefined
    const dependencies = pickValidDependencies(pkgJson.dependencies)
    if (Object.keys(dependencies).length === 0) return undefined
    return {
      hash: entryName,
      installDir,
      dependencies,
    }
  } catch {
    return undefined
  }
}

export function findGlobalPackage (globalDir: string, alias: string): GlobalPackageInfo | null {
  const packages = scanGlobalPackages(globalDir)
  return packages.find((pkg) => Object.hasOwn(pkg.dependencies, alias)) ?? null
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
  const resolvedDir = path.resolve(globalDir)
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(resolvedDir, { withFileTypes: true })
  } catch {
    return
  }

  const referenced = collectReferencedDirs(resolvedDir, entries)
  const now = Date.now()
  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    const dirPath = path.join(resolvedDir, entry.name)
    if (referenced.has(dirPath) || shouldPreserveOrphan(dirPath, now)) continue
    fs.rmSync(dirPath, { recursive: true, force: true })
  }
}

function collectReferencedDirs (globalDir: string, entries: fs.Dirent[]): Set<string> {
  const referenced = new Set<string>()
  for (const entry of entries) {
    if (!entry.isSymbolicLink()) continue
    try {
      referenced.add(fs.realpathSync(path.join(globalDir, entry.name)))
    } catch {}
  }
  return referenced
}

const SAFETY_WINDOW_MS = 5 * 60 * 1000

function shouldPreserveOrphan (dirPath: string, now: number): boolean {
  try {
    const stat = fs.statSync(dirPath)
    return now - Math.max(stat.birthtimeMs, stat.ctimeMs) < SAFETY_WINDOW_MS
  } catch {
    return true
  }
}

/** The bin names installed by a group (deduplicated). See getInstalledBins. */
export async function getInstalledBinNames (info: GlobalPackageInfo): Promise<string[]> {
  return [...new Set((await getInstalledBins(info)).map((bin) => bin.name))]
}

/**
 * The bins installed by a group, including their executable paths.
 *
 * A group whose `node_modules` is wholly absent owns no bins, and neither
 * does a declared dependency whose directory under `node_modules` is absent,
 * a link left dangling by a pruned store included: no bin can resolve
 * through a directory that is not there. Every dependency directory that
 * does exist must hold a readable, valid manifest: returning a partial set
 * would make destructive callers mistake unknown ownership for an unowned
 * bin.
 */
export async function getInstalledBins (info: GlobalPackageInfo): Promise<Command[]> {
  const bins: Command[] = []
  const aliases = Object.keys(info.dependencies)
  const modulesDir = path.join(info.installDir, 'node_modules')
  if (!await dirExists(modulesDir)) return []
  await Promise.all(
    aliases.map(async (alias) => {
      const depDir = path.join(modulesDir, alias)
      let manifest: PackageManifest
      try {
        manifest = await readPackageJsonFromDir(depDir)
      } catch (err) {
        // Probing after the read rather than before also covers a link
        // pruned while the scan runs.
        if (isNotFound(err) && !await dirExists(depDir)) return
        throw err
      }
      const binsOfPkg = await getBinsFromPackageManifest(manifest, depDir)
      for (const bin of binsOfPkg) bins.push(bin)
    })
  )
  return bins
}

/**
 * Only ENOENT reads as absent; every other error surfaces, so unreadable
 * ownership is never mistaken for unowned.
 */
async function dirExists (dir: string): Promise<boolean> {
  try {
    await fs.promises.stat(dir)
    return true
  } catch (err) {
    if (isNotFound(err)) return false
    throw err
  }
}

function isNotFound (err: unknown): boolean {
  return isError(err) && 'code' in err && err.code === 'ENOENT'
}
