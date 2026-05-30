import path from 'node:path'

import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import type { Resolution, TarballResolution } from '@pnpm/resolving.resolver-base'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { pickStoreIndexKey, type StoreIndex } from '@pnpm/store.index'

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
 * Picks the store key by resolution shape:
 * - Directory packages: fetches the file list from the local directory.
 * - Git-hosted tarballs (codeload.github.com / gitlab.com / bitbucket.org):
 *   keyed by `gitHostedStoreIndexKey(packageId, { built: true })`. The
 *   lockfile pins their integrity for security, but the cached payload
 *   depends on whether build scripts ran during fetch (preparePackage), so
 *   the `built` dimension is part of the key. Folding them under the
 *   integrity-only key would collapse that distinction.
 * - npm-registry tarballs with integrity: keyed by
 *   `storeIndexKey(integrity, packageId)`.
 * - Other tarball / git resolutions without integrity: keyed by
 *   `gitHostedStoreIndexKey(packageId, { built: true })`.
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

  let pkgIndexFilePath: string
  if (
    (!packageResolution.type && 'tarball' in packageResolution && packageResolution.tarball) ||
    packageResolution.type === 'git'
  ) {
    pkgIndexFilePath = pickStoreIndexKey(
      packageResolution as TarballResolution,
      packageId,
      { built: true }
    )
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
