import path from 'path'
import pathAbsolute from 'path-absolute'
import { readFile } from 'fs/promises'
import { readPackageJson } from '@pnpm/read-package-json'
import { depPathToFilename } from 'dependency-path'
import pLimit from 'p-limit'
import { PackageManifest, Registries } from '@pnpm/types'
import {
  getFilePathByModeInCafs,
  getFilePathInCafs,
  PackageFileInfo,
  PackageFilesIndex,
} from '@pnpm/cafs'
import loadJsonFile from 'load-json-file'
import { PnpmError } from '@pnpm/error'
import { LicensePackage } from './licenses'
import { DirectoryResolution, PackageSnapshot, pkgSnapshotToResolution, Resolution } from '@pnpm/lockfile-utils'
import { fetchFromDir } from '@pnpm/directory-fetcher'

const limitPkgReads = pLimit(4)

export async function readPkg (pkgPath: string) {
  return limitPkgReads(async () => readPackageJson(pkgPath))
}

/**
 * @const
 * List of typical names for license files
 */
const LICENSE_FILES = [
  'LICENSE',
  'LICENCE',
  'LICENSE.md',
  'LICENCE.md',
  'LICENSE.txt',
  'LICENCE.txt',
  'MIT-LICENSE.txt',
  'MIT-LICENSE.md',
  'MIT-LICENSE',
]

export interface LicenseInfo {
  name: string
  licenseFile?: string
}

/**
 * Coerce the given value to a string or a null value
 * @param field the string to be converted
 * @returns string | null
 */
function coerceToString (field: unknown): string | null {
  const string = String(field)
  return typeof field === 'string' || field === string ? string : null
}

/**
 * Parses the value of the license-property of a
 * package manifest and return it as a string
 * @param field the value to parse
 * @returns string
 */
function parseLicenseManifestField (field: unknown) {
  if (Array.isArray(field)) {
    const licenses = field
    const licenseTypes = licenses.reduce((listOfLicenseTypes, license) => {
      const type = coerceToString(license.type) ?? coerceToString(license.name)
      if (type) {
        listOfLicenseTypes.push(type)
      }
      return listOfLicenseTypes
    }, [])

    if (licenseTypes.length > 1) {
      const combinedLicenseTypes = licenseTypes.join(' OR ') as string
      return `(${combinedLicenseTypes})`
    }

    return licenseTypes[0] ?? null
  } else {
    return (field as { type: string })?.type ?? coerceToString(field)
  }
}

/**
 * Reads the license field or LICENSE file from
 * the directory of the given package manifest
 *
 * If the package.json file is missing the `license`-property
 * the root of the manifest directory will be scanned for
 * files named listed in the array LICENSE_FILES and the
 * contents will be returned.
 *
 * @param {*} pkg the package to check
 * @param {*} opts the options for parsing licenses
 * @returns Promise<LicenseInfo>
 */
async function parseLicense (
  pkg: {
    manifest: PackageManifest
    files:
    | { local: true, files: Record<string, string> }
    | { local: false, files: Record<string, PackageFileInfo> }
  },
  opts: { cafsDir: string }
): Promise<LicenseInfo> {
  let licenseField: unknown = pkg.manifest.license
  if ('licenses' in pkg.manifest) {
    licenseField = (
      pkg.manifest as PackageManifest & {
        licenses: unknown
      }
    ).licenses
  }
  const license = parseLicenseManifestField(licenseField)

  // check if we discovered a license, if not attempt to parse the LICENSE file
  if (!license || /see license/i.test(license)) {
    const { files: pkgFileIndex } = pkg.files
    for (const filename of LICENSE_FILES) {
      // check if the a file with the expected name exists in the file index
      if (!(filename in pkgFileIndex)) {
        continue
      }

      const licensePackageFileInfo = pkgFileIndex[filename]
      let licenseContents: Buffer | undefined
      if (pkg.files.local) {
        licenseContents = await readFile(licensePackageFileInfo as string)
      } else {
        licenseContents = await readLicenseFileFromCafs(
          opts.cafsDir,
          (licensePackageFileInfo as PackageFileInfo).integrity
        )
      }

      return {
        name: 'Unknown',
        licenseFile: licenseContents?.toString('utf-8'),
      }
    }
  }

  return { name: license ?? 'Unknown' }
}

/**
 * Fetch a file by integrity id from the content-addressable store
 * @param cafsDir the cafs directory
 * @param opts the options for reading file
 * @returns Promise<Buffer>
 */
async function readLicenseFileFromCafs (cafsDir: string, fileIntegrity: string) {
  const fileName = getFilePathByModeInCafs(cafsDir, fileIntegrity, 0)
  const fileContents = await readFile(fileName)
  return fileContents
}

/**
 * Returns the index of files included in
 * the package identified by the integrity id
 * @param packageResolution the resolution package information
 * @param depPath the package reference
 * @param opts options for fetching package file index
 */
export async function readPackageIndexFile (
  packageResolution: Resolution,
  depPath: string,
  opts: { cafsDir: string, storeDir: string, lockfileDir: string }
): Promise<
  | {
    local: false
    files: Record<string, PackageFileInfo>
  }
  | {
    local: true
    files: Record<string, string>
  }
  > {
  const isPackageWithIntegrity = 'integrity' in packageResolution

  let pkgIndexFilePath
  if (isPackageWithIntegrity) {
    // Retrieve all the index file of all files included in the package
    pkgIndexFilePath = await getFilePathInCafs(
      opts.cafsDir,
      packageResolution.integrity as string,
      'index'
    )
  } else if (!packageResolution.type && packageResolution.tarball) {
    // If the package resolution has a tarball then we need to clean up
    // the call to depPathToFilename as it adds '_[hash]' part to the
    // directory for the package in the content-addressable store
    const packageDirInStore = depPathToFilename(depPath.split('_')[0])
    pkgIndexFilePath = path.join(
      opts.storeDir,
      packageDirInStore,
      'integrity.json'
    )
  } else {
    throw new PnpmError(
      'UNSUPPORTED_PACKAGE_TYPE',
      `Unsupported package resolution type for ${depPath}`
    )
  }

  // If the package resolution is of type directory we need to do things
  // differently and generate our own package index file
  const isLocalPkg = packageResolution.type === 'directory'
  if (isLocalPkg) {
    const localInfo = await fetchFromDir(
      path.join(opts.lockfileDir, (packageResolution as DirectoryResolution).directory),
      {}
    )
    return {
      local: true,
      files: localInfo.filesIndex,
    }
  } else {
    try {
      const { files } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
      return {
        local: false,
        files,
      }
    } catch (err: any) {  // eslint-disable-line
      if (err.code === 'ENOENT') {
        throw new PnpmError(
          'MISSING_PACKAGE_INDEX_FILE',
          `Failed to find package index file for ${depPath}, please consider running 'pnpm install'`
        )
      }

      throw err
    }
  }
}

export interface PackageInfo {
  name?: string
  version?: string
  depPath: string
  snapshot: PackageSnapshot
  registries: Registries
}

export interface GetPackageInfoOptions {
  storeDir: string
  virtualStoreDir: string
  dir: string
  modulesDir: string
}

/**
 * Returns the package manifest information for a give package name and path
 * @param pkg the package to fetch information for
 * @param opts the fetching options
 * @returns Promise<{ from: string; description?: string } & Omit<LicensePackage, 'belongsTo'>>
 */
export async function getPkgInfo (
  pkg: PackageInfo,
  opts: GetPackageInfoOptions
): Promise<
  {
    from: string
    description?: string
  } & Omit<LicensePackage, 'belongsTo'>
  > {
  const cafsDir = path.join(opts.storeDir, 'files')

  // Retrieve file index for the requested package
  const packageResolution = pkgSnapshotToResolution(
    pkg.depPath,
    pkg.snapshot,
    pkg.registries
  )

  const packageFileIndexInfo = await readPackageIndexFile(
    packageResolution as Resolution,
    pkg.depPath,
    {
      cafsDir,
      storeDir: opts.storeDir,
      lockfileDir: opts.dir,
    }
  )

  // Fetch the package manifest
  let packageManifestDir!: string
  if (packageFileIndexInfo.local) {
    packageManifestDir = packageFileIndexInfo.files['package.json']
  } else {
    const packageFileIndex = packageFileIndexInfo.files as Record<
    string,
    PackageFileInfo
    >
    const packageManifestFile = packageFileIndex['package.json']
    packageManifestDir = await getFilePathByModeInCafs(
      cafsDir,
      packageManifestFile.integrity,
      packageManifestFile.mode
    )
  }

  let manifest
  try {
    manifest = await readPkg(packageManifestDir)
  } catch (err: any) {  // eslint-disable-line
    if (err.code === 'ENOENT') {
      throw new PnpmError(
        'MISSING_PACKAGE_MANIFEST',
        `Failed to find package manifest file at ${packageManifestDir}`
      )
    }
    throw err
  }

  // Determine the path to the package as known by the user
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const virtualStoreDir = pathAbsolute(
    opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'),
    opts.dir
  )

  // TODO: fix issue that path is only correct when using node-linked=isolated
  const packageModulePath = path.join(
    virtualStoreDir,
    depPathToFilename(pkg.depPath),
    modulesDir,
    manifest.name
  )

  const licenseInfo = await parseLicense(
    { manifest, files: packageFileIndexInfo },
    { cafsDir }
  )

  const packageInfo = {
    from: manifest.name,
    path: packageModulePath,
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    license: licenseInfo.name,
    licenseContents: licenseInfo.licenseFile,
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
