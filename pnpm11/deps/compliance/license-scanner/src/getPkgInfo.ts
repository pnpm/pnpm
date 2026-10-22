import fs from 'node:fs'
import path from 'node:path'

import { resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { depPathToFilename, findHoistedPackageDirs } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { type PackageSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { PackageManifest, RegistriesByScope, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'
import { pathAbsolute } from 'path-absolute'

import type { LicensePackage } from './licenses.js'

const limitPkgReads = pLimit(4)

export async function readPkg (pkgPath: string): Promise<PackageManifest> {
  return limitPkgReads(async () => readPackageJson(pkgPath))
}

export interface PackageInfo {
  id: string
  name?: string
  version?: string
  depPath: string
  snapshot: PackageSnapshot
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
}

export interface GetPackageInfoOptions {
  storeDir: string
  storeIndex: StoreIndex
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  dir: string
  lockfileDir?: string
  modulesDir: string
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  shamefullyHoist?: boolean
  /**
   * Lockfile-relative directories keyed by dependency path, recorded by a
   * `nodeLinker: hoisted` install, which leaves the virtual store empty.
   */
  hoistedLocations?: Record<string, string[]>
  supportedArchitectures?: SupportedArchitectures
}

export type PkgInfo = {
  from: string
  description?: string
} & Omit<LicensePackage, 'belongsTo'>

/**
 * Returns the package manifest information for a give package name and path
 * @param pkg the package to fetch information for
 * @param opts the fetching options
 */
export async function getPkgInfo (
  pkg: PackageInfo,
  opts: GetPackageInfoOptions
): Promise<PkgInfo> {
  const { files, manifest } = await readPackageFilesAndManifest(pkg, opts)
  const { packageModulePath, hoistedPaths } = await resolvePackageModulePath(pkg, opts, manifest)
  const licenseInfo = await resolveLicense({ manifest, files })

  return {
    from: manifest.name,
    path: packageModulePath,
    ...(hoistedPaths.length ? { paths: [...new Set(hoistedPaths)] } : {}),
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    license: licenseInfo?.name ?? 'Unknown',
    licenseContents: licenseInfo?.licenseFile,
    author: extractAuthor(manifest),
    homepage: manifest.homepage,
    repository: extractRepository(manifest),
  }
}

async function readPackageFilesAndManifest (
  pkg: PackageInfo,
  opts: GetPackageInfoOptions
): Promise<{ files: Map<string, string>, manifest: PackageManifest }> {
  const packageResolution = pkgSnapshotToResolution(
    pkg.depPath,
    pkg.snapshot,
    { registriesByScope: pkg.registriesByScope, registriesByPrefix: pkg.registriesByPrefix }
  )

  let files: Map<string, string>
  try {
    const result = await readPackageFileMap(
      packageResolution,
      pkg.id,
      {
        storeDir: opts.storeDir,
        storeIndex: opts.storeIndex,
        lockfileDir: opts.lockfileDir ?? opts.dir,
        supportedArchitectures: opts.supportedArchitectures,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      }
    )
    if (!result) {
      throw new PnpmError('UNSUPPORTED_PACKAGE_TYPE', `Unsupported package resolution type for ${pkg.id}`)
    }
    files = result
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') {
      throw new PnpmError(
        'MISSING_PACKAGE_INDEX_FILE',
        `Failed to find package index file for ${pkg.id} (at ${err.path}), please consider running 'pnpm install'`
      )
    }
    throw err
  }

  const manifestPath = files.get('package.json')
  if (!manifestPath) {
    throw new PnpmError(
      'MISSING_PACKAGE_INDEX_FILE',
      `Failed to find package.json in index for ${pkg.id}, please consider running 'pnpm install'`
    )
  }
  const manifest = await readPackageJson(manifestPath)
  return { files, manifest }
}

async function resolvePackageModulePath (
  pkg: PackageInfo,
  opts: GetPackageInfoOptions,
  manifest: PackageManifest
): Promise<{ packageModulePath: string, hoistedPaths: string[] }> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), lockfileDir)
  const virtualStorePath = path.join(
    virtualStoreDir,
    depPathToFilename(pkg.depPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    manifest.name
  )

  const hoistedPaths = findHoistedPackageDirs(opts.hoistedLocations, pkg.depPath, lockfileDir)
  if (hoistedPaths.length) {
    return { packageModulePath: pickBestHoistedPath(hoistedPaths, opts.dir), hoistedPaths }
  }

  if (opts.nodeLinker === 'hoisted') {
    const pathInHoisted = await findCandidateInHoisted(opts.dir, lockfileDir, modulesDir, manifest)
    return { packageModulePath: pathInHoisted ?? virtualStorePath, hoistedPaths }
  }

  if (opts.shamefullyHoist) {
    const pathInShamefullyHoist = await findCandidateInShameful(opts.dir, lockfileDir, modulesDir, manifest.name, virtualStorePath)
    return { packageModulePath: pathInShamefullyHoist ?? virtualStorePath, hoistedPaths }
  }

  return { packageModulePath: virtualStorePath, hoistedPaths }
}

function pickBestHoistedPath (hoistedPaths: string[], dir: string): string {
  const resolvedDir = path.resolve(dir)
  const dirWithSep = resolvedDir.endsWith(path.sep) ? resolvedDir : resolvedDir + path.sep
  return (
    hoistedPaths.find((loc) => (loc === resolvedDir || loc.startsWith(dirWithSep)) && fs.existsSync(loc)) ??
    hoistedPaths.find((loc) => fs.existsSync(loc)) ??
    hoistedPaths[0]
  )
}

async function findCandidateInHoisted (
  dir: string,
  lockfileDir: string,
  modulesDir: string,
  manifest: PackageManifest
): Promise<string | undefined> {
  const candidateInDir = path.resolve(dir, modulesDir, manifest.name)
  if (await candidateMatchesVersion(candidateInDir, manifest.version)) return candidateInDir
  const candidateInLockfileDir = path.resolve(lockfileDir, modulesDir, manifest.name)
  if (await candidateMatchesVersion(candidateInLockfileDir, manifest.version)) return candidateInLockfileDir
  return undefined
}

async function findCandidateInShameful (
  dir: string,
  lockfileDir: string,
  modulesDir: string,
  name: string,
  virtualStorePath: string
): Promise<string | undefined> {
  const candidateInDir = path.resolve(dir, modulesDir, name)
  if (await matchesVirtualStore(candidateInDir, virtualStorePath)) return candidateInDir
  const candidateInLockfileDir = path.resolve(lockfileDir, modulesDir, name)
  if (await matchesVirtualStore(candidateInLockfileDir, virtualStorePath)) return candidateInLockfileDir
  return undefined
}

function extractAuthor (manifest: PackageManifest): string | undefined {
  if (!manifest.author) return undefined
  if (typeof manifest.author === 'string') return manifest.author
  return (manifest.author as { name: string }).name
}

function extractRepository (manifest: PackageManifest): string | undefined {
  if (!manifest.repository) return undefined
  if (typeof manifest.repository === 'string') return manifest.repository
  return manifest.repository.url
}

async function matchesVirtualStore (candidate: string, expectedVirtualStorePath: string): Promise<boolean> {
  try {
    const [realCandidate, realExpected] = await Promise.all([
      fs.promises.realpath(candidate),
      fs.promises.realpath(expectedVirtualStorePath),
    ])
    return realCandidate === realExpected
  } catch {
    return false
  }
}

async function candidateMatchesVersion (candidate: string, expectedVersion: string | undefined): Promise<boolean> {
  if (!expectedVersion) return fs.existsSync(candidate)
  try {
    const manifest = await readPackageJson(path.join(candidate, 'package.json'))
    return manifest.version === expectedVersion
  } catch {
    return false
  }
}
