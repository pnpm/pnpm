import { access, readFile } from 'node:fs/promises'
import path from 'node:path'

import { type ManifestWithLicense, parseLicenseFromManifest } from './parseLicenseFromManifest.js'

export * from './parseLicenseFromManifest.js'

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
 * The subset of {@link LICENSE_NAMES} that are valid SPDX license identifiers.
 * Consumers needing spec-compliant output (e.g. SPDX SBOM) should drop any
 * other detected name — see {@link isSpdxLicenseExpression}.
 */
export const SPDX_LICENSE_IDS = new Set([
  'Apache-1.1',
  'Apache-2.0',
  'BSD-4-Clause',
  'BSD-3-Clause',
  'BSD-2-Clause',
  '0BSD',
  'CC0-1.0',
  'CDDL-1.0',
  'EPL-1.0',
  'GPL-2.0-only',
  'GPL-3.0-only',
  'ISC',
  'LGPL-3.0-only',
  'LGPL-2.1-only',
  'MIT',
  'MPL-1.1',
  'MPL-2.0',
  'OFL-1.1',
  'PSF-2.0',
  'WTFPL',
  'Zlib',
])

/**
 * Returns true when `value` is a single SPDX ID from {@link SPDX_LICENSE_IDS}
 * or a parenthesized `OR` expression of such IDs (e.g. `(MIT OR Apache-2.0)`).
 */
export function isSpdxLicenseExpression (value: string): boolean {
  if (!value) return false
  return value
    .replace(/^\(|\)$/g, '')
    .split(/\s+OR\s+/i)
    .every(id => SPDX_LICENSE_IDS.has(id.trim()))
}

/**
 * License names detected by scanning LICENSE file contents. Includes both SPDX
 * identifiers and common long-form names — the latter are useful for the
 * human-facing `pnpm licenses` command but are not valid SPDX output.
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

export interface ResolveLicenseFromDirInput {
  manifest: ManifestWithLicense
  /** Absolute path to the directory containing the package's files. */
  dir: string
}

/**
 * Like {@link resolveLicense} but scans a directory on disk for LICENSE files
 * instead of consulting a pre-built file map. Useful for the local project
 * root where no store index is available.
 */
export async function resolveLicenseFromDir ({ manifest, dir }: ResolveLicenseFromDirInput): Promise<ResolvedLicense | undefined> {
  const files = new Map<string, string>()
  await Promise.all(LICENSE_FILES.map(async (name) => {
    const filePath = path.join(dir, name)
    try {
      await access(filePath)
      files.set(name, filePath)
    } catch {
      // File doesn't exist — skip.
    }
  }))
  return resolveLicense({ manifest, files })
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
