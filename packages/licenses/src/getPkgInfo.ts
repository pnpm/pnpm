import path from 'node:path'
import fs from 'node:fs/promises'
import { readPackageJson } from '@pnpm/read-package-json'
import pLimit from 'p-limit'
import { PackageManifest } from '@pnpm/types'

const limitPkgReads = pLimit(4)

export async function readPkg (pkgPath: string) {
  return limitPkgReads(async () => readPackageJson(pkgPath))
}

/**
 * @const
 * List of typical names for license files
 */
const LICENSE_FILES = ['./LICENSE', './LICENCE']

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
 *
 * @param field
 * @returns
 */
function parseLicenseManifestField (field: unknown) {
  if (Array.isArray(field)) {
    const licenses = field
    const licenseTypes = licenses.reduce((listOfLicenseTypes, license) => {
      const type = coerceToString(license.type)
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
 *
 * @param {*} packageInfo
 * @returns
 */
async function parseLicense (packageInfo: {
  manifest: PackageManifest
  path: string
}): Promise<LicenseInfo> {
  const license = parseLicenseManifestField(packageInfo.manifest.license)

  // check if we discovered a license, if not attempt to parse the LICENSE file
  if (
    (!license || /see license/i.test(license))
  ) {
    for (const filename of LICENSE_FILES) {
      try {
        const licensePath = path.join(packageInfo.path, filename)
        const licenseContents = await fs.readFile(licensePath)
        return {
          name: 'Unknown',
          licenseFile: licenseContents.toString('utf-8'),
        }
      } catch (err) {
        // NOOP
      }
    }
  }

  return { name: license ?? 'Unknown' }
}

/**
 *
 * @param pkg
 * @returns
 */
export async function getPkgInfo (
  pkg: {
    name: string
    version: string
    prefix: string
  }
): Promise<{
    packageManifest: PackageManifest
    packageInfo: {
      from: string
      path: string
      version: string
      description?: string
      license: string
      licenseContents?: string
      author?: string
      homepage?: string
      repository?: string
    }
  }> {
  let manifest
  let packageModulePath
  let licenseInfo: LicenseInfo

  if (pkg.name.length === 0) {
    throw new Error('Missing package name')
  }

  try {
    packageModulePath = path.join(pkg.prefix, pkg.name)
    const packageManifestPath = path.join(packageModulePath, 'package.json')
    manifest = await readPkg(packageManifestPath)

    licenseInfo = await parseLicense({ manifest, path: packageModulePath })

    manifest.license = licenseInfo.name
  } catch (err: unknown) {
    // This will probably never happen
    throw new Error(`Failed to fetch manifest data for ${pkg.name}`)
  }

  return {
    packageManifest: manifest,
    packageInfo: {
      from: pkg.name,
      path: packageModulePath,
      version: pkg.version,
      description: manifest.description,
      license: licenseInfo.name,
      licenseContents: licenseInfo.licenseFile,
      author: (manifest.author && (
        typeof manifest.author === 'string' ? manifest.author : (manifest.author as { name: string }).name
      )) ?? undefined,
      homepage: manifest.homepage,
      repository: (manifest.repository && (
        typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
      )) ?? undefined,
    },
  }
}
