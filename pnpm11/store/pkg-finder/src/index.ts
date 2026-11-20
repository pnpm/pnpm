import path from 'node:path'

import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import {
  type AtomicResolution,
  type Resolution,
  resolvePlatformSelector,
  selectPlatformVariant,
  type TarballResolution,
} from '@pnpm/resolving.resolver-base'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { pickStoreIndexKey, type StoreIndex } from '@pnpm/store.index'
import type { SupportedArchitectures } from '@pnpm/types'
import { familySync } from 'detect-libc'

export interface ReadPackageFileMapOptions {
  storeDir: string
  storeIndex: StoreIndex
  lockfileDir: string
  supportedArchitectures?: SupportedArchitectures
  virtualStoreDirMaxLength: number
  /**
   * Decoded package-file indexes keyed by store index key, shared across one
   * traversal. When several depPaths resolve to the same store entry, only the
   * first read hits the store. The caller owns the map's lifetime; entries are
   * never invalidated, so drop the map when the traversal ends.
   */
  indexCache?: PackageFilesIndexCache
}

export type PackageFilesIndexCache = Map<string, PackageFilesIndex>

/**
 * Reads the file index for a package and returns a `Map<string, string>`
 * mapping filenames to absolute paths on disk.
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
  const selectedResolution = selectPackageResolution(packageResolution, opts.supportedArchitectures)
  if (!selectedResolution) return undefined

  if (selectedResolution.type === 'directory') {
    const localInfo = await fetchFromDir(
      path.join(opts.lockfileDir, selectedResolution.directory),
      {}
    )
    return localInfo.filesMap
  }

  let pkgIndexFilePath: string
  if (
    (!selectedResolution.type && 'tarball' in selectedResolution && selectedResolution.tarball) ||
    selectedResolution.type === 'git' ||
    selectedResolution.type === 'binary'
  ) {
    pkgIndexFilePath = pickStoreIndexKey(
      selectedResolution as TarballResolution,
      packageId,
      { built: true }
    )
  } else {
    return undefined
  }

  const { files: indexFiles } = readPackageFilesIndex(pkgIndexFilePath, opts)
  const files = new Map<string, string>()
  for (const [name, info] of indexFiles) {
    files.set(name, getFilePathByModeInCafs(opts.storeDir, info.digest, info.mode))
  }
  return files
}

function readPackageFilesIndex (
  pkgIndexFilePath: string,
  opts: Pick<ReadPackageFileMapOptions, 'storeIndex' | 'indexCache'>
): PackageFilesIndex {
  const cached = opts.indexCache?.get(pkgIndexFilePath)
  if (cached != null) return cached
  const pkgFilesIndex = opts.storeIndex.get(pkgIndexFilePath) as PackageFilesIndex | undefined
  if (!pkgFilesIndex) {
    const err: NodeJS.ErrnoException = new Error(
      `ENOENT: package index not found for '${pkgIndexFilePath}'`
    )
    err.code = 'ENOENT'
    err.path = pkgIndexFilePath
    throw err
  }
  opts.indexCache?.set(pkgIndexFilePath, pkgFilesIndex)
  return pkgFilesIndex
}

function selectPackageResolution (
  resolution: Resolution,
  supportedArchitectures: SupportedArchitectures | undefined
): AtomicResolution | undefined {
  if (resolution.type !== 'variations') return resolution
  const selector = resolvePlatformSelector(supportedArchitectures, {
    platform: process.platform,
    arch: process.arch,
    libc: familySync(),
  })
  return selectPlatformVariant(resolution.variants, selector)?.resolution
}
