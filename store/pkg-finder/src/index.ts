import path from 'path'
import { depPathToFilename, parse } from '@pnpm/dependency-path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { readMsgpackFile } from '@pnpm/fs.msgpack-file'
import { type Resolution } from '@pnpm/resolver-base'
import { getFilePathByModeInCafs, getIndexFilePathInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'

export interface ReadPackageFileMapOptions {
  storeDir: string
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

  const isPackageWithIntegrity = 'integrity' in packageResolution

  let pkgIndexFilePath: string
  if (isPackageWithIntegrity) {
    const parsedId = parse(packageId)
    pkgIndexFilePath = getIndexFilePathInCafs(
      opts.storeDir,
      packageResolution.integrity as string,
      parsedId.nonSemverVersion ?? `${parsedId.name}@${parsedId.version}`
    )
  } else if (!packageResolution.type && 'tarball' in packageResolution && packageResolution.tarball) {
    const packageDirInStore = depPathToFilename(parse(packageId).nonSemverVersion ?? packageId, opts.virtualStoreDirMaxLength)
    pkgIndexFilePath = path.join(
      opts.storeDir,
      packageDirInStore,
      'integrity.mpk'
    )
  } else {
    return undefined
  }

  const { files: indexFiles } = await readMsgpackFile<PackageFilesIndex>(pkgIndexFilePath)
  const files = new Map<string, string>()
  for (const [name, info] of indexFiles) {
    files.set(name, getFilePathByModeInCafs(opts.storeDir, info.digest, info.mode))
  }
  return files
}
