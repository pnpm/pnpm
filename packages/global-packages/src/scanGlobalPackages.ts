import fs from 'fs'
import path from 'path'
import util from 'util'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { readPackageJsonFromDir, safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type PackageManifest } from '@pnpm/types'
import { resolveActiveInstall } from './globalPackageDir.js'

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
    if (!entry.isDirectory()) continue
    const hashDir = path.join(globalDir, entry.name)
    const installDir = resolveActiveInstall(hashDir)
    if (!installDir) continue
    const pkgJsonPath = path.join(installDir, 'package.json')
    let pkgJson: { dependencies?: Record<string, string> }
    try {
      pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf-8'))
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
