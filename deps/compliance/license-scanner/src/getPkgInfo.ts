import path from 'node:path'

import { resolveLicense } from '@pnpm/deps.compliance.license-resolver'
import { depPathToFilename } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { type PackageSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import { readPackageJson } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'
import type { PackageManifest, Registries } from '@pnpm/types'
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
  registries: Registries
}

export interface GetPackageInfoOptions {
  storeDir: string
  storeIndex: StoreIndex
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  dir: string
  modulesDir: string
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
    pkg.registries
  )

  let files: Map<string, string>
  try {
    const result = await readPackageFileMap(
      packageResolution,
      pkg.id,
      {
        storeDir: opts.storeDir,
        storeIndex: opts.storeIndex,
        lockfileDir: opts.dir,
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
  const virtualStoreDir = pathAbsolute(
    opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'),
    opts.dir
  )

  // TODO: fix issue that path is only correct when using node-linked=isolated
  const packageModulePath = path.join(
    virtualStoreDir,
    depPathToFilename(pkg.depPath, opts.virtualStoreDirMaxLength),
    modulesDir,
    manifest.name
  )

  const licenseInfo = await resolveLicense({ manifest, files })

  const packageInfo = {
    from: manifest.name,
    path: packageModulePath,
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
