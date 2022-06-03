import * as path from 'node:path'
import * as fs from 'node:fs/promises'
import { LicenseInfo, LicensePredicate } from './types'
import { ProjectManifest } from '@pnpm/types'

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
 * Parse the license field of a package manifest
 * @param {*} field the value of the license manifest
 * @returns string | null
 */
function parseLicenseManifestField (field: unknown): string | null {
  if (Array.isArray(field)) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
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
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (field as any)?.type ?? coerceToString(field)
}

/**
 * @const
 * List of typical names for license files
 */
const LICENSE_FILES = ['./LICENSE', './LICENCE']

/**
 *
 * @param {*} packageInfo
 * @returns
 */
export async function parseLicense (packageInfo: {
  manifest: ProjectManifest
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
        // eslint-disable-next-line no-await-in-loop
        const licenseContents = await fs.readFile(licensePath)
        return {
          licenseFile: licenseContents.toString('utf-8'),
        }
      } catch (err) {
        // NOOP
      }
    }
  }

  return { license: license as string }
}

/**
 * @internal
 * Returns whether the license is missing or not
 * @param license
 * @returns Promise<boolean>
 */
async function isMissing ({ license }: { license: string }): Promise<boolean> {
  const pattern = /\\b(unknown|see license)\\b/i
  return pattern.test(license)
}

export async function isAllowableLicense ({
  license,
  isFile,
  isValidLicensePredicate,
}: {
  license: string | null
  isFile: boolean
  isValidLicensePredicate: LicensePredicate
}) {
  if (license && !(await isMissing({ license }))) {
    if (isValidLicensePredicate(license, isFile)) {
      return { pass: true }
    }
    return { pass: false, reason: 'incompatible' }
  }
  return { reason: 'missing', pass: false }
}
