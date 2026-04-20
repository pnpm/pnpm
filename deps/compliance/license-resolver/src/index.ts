import { readFile } from 'node:fs/promises'

import { type ManifestWithLicense, parseLicenseFromManifest } from '@pnpm/pkg-manifest.utils'

/** Filenames commonly used to ship a package's license text. Checked in order. */
export const LICENSE_FILES = [
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

/**
 * License names detected by scanning LICENSE file contents. Includes both SPDX
 * identifiers and common long-form names.
 * Reference: https://github.com/pivotal/LicenseFinder/blob/master/lib/license_finder/license/definitions.rb
 */
export const LICENSE_NAMES = [
  'Apache1_1',
  'Apache-1.1',
  'Apache 1.1',
  'Apache2',
  'Apache-2.0',
  'Apache 2.0',
  'BSD',
  'BSD-4-Clause',
  'CC01',
  'CC0-1.0',
  'CC0 1.0',
  'CDDL1',
  'CDDL-1.0',
  'Common Development and Distribution License 1.0',
  'EPL1',
  'EPL-1.0',
  'Eclipse Public License 1.0',
  'GPLv2',
  'GPL-2.0-only',
  'GPLv3',
  'GPL-3.0-only',
  'ISC',
  'LGPL',
  'LGPL-3.0-only',
  'LGPL2_1',
  'LGPL-2.1-only',
  'MIT',
  'MPL1_1',
  'MPL-1.1',
  'Mozilla Public License 1.1',
  'MPL2',
  'MPL-2.0',
  'Mozilla Public License 2.0',
  'NewBSD',
  'BSD-3-Clause',
  'New BSD',
  'OFL',
  'OFL-1.1',
  'SIL OPEN FONT LICENSE Version 1.1',
  'Python',
  'PSF-2.0',
  'Python Software Foundation License',
  'Ruby',
  'SimplifiedBSD',
  'BSD-2-Clause',
  'Simplified BSD',
  'WTFPL',
  '0BSD',
  'BSD Zero Clause License',
  'Zlib',
  'zlib/libpng license',
]

export interface ResolvedLicense {
  /** The detected license identifier / expression. */
  name: string
  /** The raw contents of the scanned LICENSE file, if one was used. */
  licenseFile?: string
}

export interface ResolveLicenseInput {
  manifest: ManifestWithLicense
  /**
   * Map of filename → absolute path on disk for the package's published files.
   * Usually the result of `readPackageFileMap` from `@pnpm/store.pkg-finder`.
   */
  files: Map<string, string>
}

/**
 * Derive a single license for a package, consulting (in order):
 *
 *  1. The manifest's `license` field, then its deprecated `licenses` array
 *     (via {@link parseLicenseFromManifest}).
 *  2. If the manifest is missing license info, or uses the `SEE LICENSE IN …`
 *     sentinel, a LICENSE file on disk — whose contents are regex-matched
 *     against well-known license names to produce an SPDX-ish identifier.
 *
 * Returns `undefined` when no license can be determined. Callers decide how
 * to represent "unknown": the license scanner uses the string `"Unknown"`;
 * SBOM emits SPDX `NOASSERTION`.
 */
export async function resolveLicense ({ manifest, files }: ResolveLicenseInput): Promise<ResolvedLicense | undefined> {
  const manifestLicense = parseLicenseFromManifest(manifest)

  if (manifestLicense && !/see license/i.test(manifestLicense)) {
    return { name: manifestLicense }
  }

  const licenseFileName = LICENSE_FILES.find(f => files.has(f))
  if (licenseFileName) {
    const licenseFilePath = files.get(licenseFileName)!
    const licenseContent = (await readFile(licenseFilePath)).toString('utf-8')
    const name = detectLicenseFromText(licenseContent) ?? 'Unknown'
    return { name, licenseFile: licenseContent }
  }

  return manifestLicense ? { name: manifestLicense } : undefined
}

const LICENSE_NAME_PATTERN = new RegExp(
  `\\b(${LICENSE_NAMES.map(escapeRegExp).join('|')})\\b`,
  'gi'
)

function detectLicenseFromText (content: string): string | undefined {
  if (!content) return undefined
  const match = content.match(LICENSE_NAME_PATTERN)
  if (!match) return undefined
  return [...new Set(match)].join(' OR ')
}

function escapeRegExp (value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}
