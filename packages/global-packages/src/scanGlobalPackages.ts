import fs from 'fs'
import path from 'path'
import util from 'util'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type PackageManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'

export interface GlobalPackageInfo {
  hash: string
  installDir: string
  dependencies: Record<string, string>
}

export interface GlobalPackageDetail extends GlobalPackageInfo {
  installedPackages: Array<{
    alias: string
    version: string
    manifest: PackageManifest
  }>
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
    const pkgJsonPath = path.join(installDir, 'package.json')
    let pkgJson: { dependencies?: Record<string, string> }
    try {
      pkgJson = loadJsonFileSync<{ dependencies?: Record<string, string> }>(pkgJsonPath)
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

export async function getGlobalPackageDetails (info: GlobalPackageInfo): Promise<GlobalPackageDetail> {
  const aliases = Object.keys(info.dependencies)
  const manifests = await Promise.all(
    aliases.map((alias) => safeReadPackageJsonFromDir(path.join(info.installDir, 'node_modules', alias)))
  )
  const installedPackages: GlobalPackageDetail['installedPackages'] = []
  for (let i = 0; i < aliases.length; i++) {
    const manifest = manifests[i]
    if (manifest) {
      installedPackages.push({
        alias: aliases[i],
        version: manifest.version,
        manifest,
      })
    }
  }
  return {
    ...info,
    installedPackages,
  }
}

export function cleanOrphanedInstallDirs (globalDir: string): void {
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

  // Remove directories that no symlink points to
  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    const dirPath = path.join(globalDir, entry.name)
    try {
      const realPath = fs.realpathSync(dirPath)
      if (!referenced.has(realPath)) {
        fs.rmSync(dirPath, { recursive: true, force: true })
      }
    } catch {}
  }
}

export async function getInstalledBinNames (info: GlobalPackageInfo): Promise<string[]> {
  const aliases = Object.keys(info.dependencies)
  const manifests = await Promise.all(
    aliases.map((alias) => readPackageJsonFromDir(path.join(info.installDir, 'node_modules', alias)))
  )
  const binsPerPkg = await Promise.all(
    manifests.map((manifest, i) =>
      getBinsFromPackageManifest(manifest as PackageManifest, path.join(info.installDir, 'node_modules', aliases[i]))
    )
  )
  return binsPerPkg.flat().map((bin) => bin.name)
}
