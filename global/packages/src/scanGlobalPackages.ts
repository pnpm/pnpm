import fs from 'fs'
import path from 'path'
import util from 'util'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { readPackageJsonFromDirRawSync, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type PackageManifest } from '@pnpm/types'

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
    if (!pkgJson.dependencies || Object.keys(pkgJson.dependencies).length === 0) continue
    result.push({
      hash: entry.name,
      installDir,
      dependencies: pkgJson.dependencies,
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
