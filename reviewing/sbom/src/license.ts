import spdxLicenseIds from 'spdx-license-ids'

const spdxIdSet = new Set(spdxLicenseIds)

// Classifies a license string from package.json into the appropriate
// CycloneDX representation: a known SPDX ID, an SPDX expression, or a free-text name.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  if (license.includes(' OR ') || license.includes(' AND ') || license.includes(' WITH ')) {
    return { expression: license }
  }
  if (spdxIdSet.has(license)) {
    return { license: { id: license } }
  }
  return { license: { name: license } }
}
