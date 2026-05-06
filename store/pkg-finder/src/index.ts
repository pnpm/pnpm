import path from 'node:path'

import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { isGitHostedPkgUrl } from '@pnpm/fetching.pick-fetcher'
import type { Resolution, TarballResolution } from '@pnpm/resolving.resolver-base'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, type StoreIndex, storeIndexKey } from '@pnpm/store.index'

export interface ReadPackageFileMapOptions {
  storeDir: string
  storeIndex: StoreIndex
  lockfileDir: string
  virtualStoreDirMaxLength: number
}

/**
 * Reads the file index for a package and returns a `Map<string, string>`
 * mapping filenames to absolute paths on disk.
 *
 * Handles three types of package resolutions:
 * - Directory packages: fetches the file list from the local directory
 * - Packages with integrity: looks up the index file in the CAFS by integrity hash
 * - Tarball packages: looks up the index file by package directory name
 *
 * For CAFS packages, the content-addressed digests are resolved to file
 * paths upfront, so callers get a uniform map regardless of resolution type.
 *
 * Note: this function does not include files from side effects (post-install
 * scripts). Use the raw `PackageFilesIndex` if you need side-effect files.
 *
 * Returns `undefined` if the resolution type is unsupported, letting callers
 * decide how to handle this case. Throws if the index file cannot be read.
 */
export async function readPackageFileMap (
  packageResolution: Resolution,
  packageId: string,
  opts: ReadPackageFileMapOptions
): Promise<Map<string, string> | undefined> {
  if (packageResolution.type === 'directory') {
    const localInfo = await fetchFromDir(
      path.join(opts.lockfileDir, packageResolution.directory),
      {}
    )
    return localInfo.filesMap
  }

  const tarballUrl = (packageResolution as TarballResolution).tarball
  const isGitHostedTarball = tarballUrl != null && isGitHostedPkgUrl(tarballUrl)
  const isPackageWithIntegrity = 'integrity' in packageResolution

  let pkgIndexFilePath: string
  // Git-hosted tarballs are keyed by gitHostedStoreIndexKey regardless of
  // integrity. Their cached content depends on whether build scripts ran
  // (preparePackage during fetch), so the `built` dimension must be part of
  // the key. Folding them under the integrity-only key would let a not-built
  // entry serve a request that expects the built variant. The lockfile still
  // pins integrity for security and the fetcher validates it on download.
  if (isGitHostedTarball) {
    pkgIndexFilePath = gitHostedStoreIndexKey(packageId, { built: true })
  } else if (isPackageWithIntegrity) {
    // The writer in @pnpm/installing.package-requester keys the index file by
    // the resolution id (`name@version` for npm tarballs), so we must use the
    // same key here.
    pkgIndexFilePath = storeIndexKey(
      packageResolution.integrity as string,
      packageId
    )
  } else if (!packageResolution.type && 'tarball' in packageResolution && packageResolution.tarball) {
    pkgIndexFilePath = gitHostedStoreIndexKey(packageId, { built: true })
  } else if (packageResolution.type === 'git') {
    pkgIndexFilePath = gitHostedStoreIndexKey(packageId, { built: true })
  } else {
    return undefined
  }

  const pkgFilesIndex = opts.storeIndex.get(pkgIndexFilePath) as PackageFilesIndex | undefined
  if (!pkgFilesIndex) {
    const err: NodeJS.ErrnoException = new Error(
      `ENOENT: package index not found for '${pkgIndexFilePath}'`
    )
    err.code = 'ENOENT'
    err.path = pkgIndexFilePath
    throw err
  }
  const { files: indexFiles } = pkgFilesIndex
  const files = new Map<string, string>()
  for (const [name, info] of indexFiles) {
    files.set(name, getFilePathByModeInCafs(opts.storeDir, info.digest, info.mode))
  }
  return files
}
