import { SPDX } from '@cyclonedx/cyclonedx-library'

// Classifies a license string from package.json into the appropriate
// CycloneDX representation: a known SPDX ID, an SPDX expression, or a free-text name.
// Uses the CycloneDX library's own list of supported SPDX IDs for accuracy.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  if (SPDX.isSupportedSpdxId(license)) {
    return { license: { id: license } }
  }
  if (SPDX.isValidSpdxLicenseExpression(license)) {
    return { expression: license }
  }
  return { license: { name: license } }
}
