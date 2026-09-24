import fs from 'node:fs'
import path from 'node:path'

import { resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { depPathToFilename, removeSuffix } from '@pnpm/deps.path'
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
  // Retrieve file index for the requested package
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
      throw new PnpmError(
        'UNSUPPORTED_PACKAGE_TYPE',
        `Unsupported package resolution type for ${pkg.id}`
      )
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

  // Determine the path to the package as known by the user
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const isHoisted = opts.nodeLinker === 'hoisted'
  const isShamefullyHoist = opts.shamefullyHoist ?? false

  let packageModulePath: string

  const virtualStoreDir = pathAbsolute(
    opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'),
    lockfileDir
  )
  const virtualStorePath = path.join(
    virtualStoreDir,
    depPathToFilename(pkg.depPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    manifest.name
  )

  const locations = opts.hoistedLocations?.[pkg.depPath] ??
    opts.hoistedLocations?.[removeSuffix(pkg.depPath)] ??
    (pkg.depPath.startsWith('/') ? opts.hoistedLocations?.[pkg.depPath.slice(1)] : opts.hoistedLocations?.[`/${pkg.depPath}`]) ??
    []
  const hoistedPaths = locations
    .map((location) => hoistedPackageDir(lockfileDir, location))
    .filter((location): location is string => location != null)
  if (hoistedPaths.length) {
    const resolvedDir = path.resolve(opts.dir)
    const dirWithSep = resolvedDir.endsWith(path.sep) ? resolvedDir : resolvedDir + path.sep
    packageModulePath =
      hoistedPaths.find((loc) => (loc === resolvedDir || loc.startsWith(dirWithSep)) && fs.existsSync(loc)) ??
      hoistedPaths.find((loc) => fs.existsSync(loc)) ??
      hoistedPaths[0]
  } else if (isHoisted) {
    const candidateInDir = path.resolve(opts.dir, modulesDir, manifest.name)
    const candidateInLockfileDir = path.resolve(lockfileDir, modulesDir, manifest.name)
    if (await candidateMatchesVersion(candidateInDir, manifest.version)) {
      packageModulePath = candidateInDir
    } else if (await candidateMatchesVersion(candidateInLockfileDir, manifest.version)) {
      packageModulePath = candidateInLockfileDir
    } else if (fs.existsSync(virtualStorePath)) {
      packageModulePath = virtualStorePath
    } else {
      packageModulePath = candidateInLockfileDir
    }
  } else if (isShamefullyHoist) {
    const candidateInDir = path.resolve(opts.dir, modulesDir, manifest.name)
    const candidateInLockfileDir = path.resolve(lockfileDir, modulesDir, manifest.name)
    if (await matchesVirtualStore(candidateInDir, virtualStorePath)) {
      packageModulePath = candidateInDir
    } else if (await matchesVirtualStore(candidateInLockfileDir, virtualStorePath)) {
      packageModulePath = candidateInLockfileDir
    } else {
      packageModulePath = virtualStorePath
    }
  } else {
    packageModulePath = virtualStorePath
  }

  const licenseInfo = await resolveLicense({ manifest, files })

  const packageInfo = {
    from: manifest.name,
    path: packageModulePath,
    ...(hoistedPaths.length ? { paths: [...new Set(hoistedPaths)] } : {}),
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    license: licenseInfo?.name ?? 'Unknown',
    licenseContents: licenseInfo?.licenseFile,
    author:
      (manifest.author &&
        (typeof manifest.author === 'string'
          ? manifest.author
          : (manifest.author as { name: string }).name)) ??
      undefined,
    homepage: manifest.homepage,
    repository:
      (manifest.repository &&
        (typeof manifest.repository === 'string'
          ? manifest.repository
          : manifest.repository.url)) ??
      undefined,
  }

  return packageInfo
}

/**
 * A lockfile-relative hoisted location resolved against `lockfileDir`, or
 * `undefined` for a location that leaves it.
 */
function hoistedPackageDir (lockfileDir: string, location: string | undefined): string | undefined {
  if (location == null || path.isAbsolute(location)) return undefined
  const dir = path.join(lockfileDir, location)
  const relative = path.relative(lockfileDir, dir)
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) return undefined
  return dir
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
