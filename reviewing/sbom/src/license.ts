// Sub-path import to pull only the SPDX module — avoids dragging in the
// validation/serialize layers with optional native deps that break esbuild bundling.
import { isSupportedSpdxId, isValidSpdxLicenseExpression } from '@cyclonedx/cyclonedx-library/SPDX'

// Classifies a license string into the appropriate CycloneDX representation.
// Uses the CycloneDX library's own SPDX list rather than spdx-license-ids,
// since CycloneDX maintains its own subset of recognized IDs.
// Order matters: check ID first because "MIT" matches both isSupportedSpdxId
// and isValidSpdxLicenseExpression, but we prefer the more specific license.id form.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  if (isSupportedSpdxId(license)) {
    return { license: { id: license } }
  }
  if (isValidSpdxLicenseExpression(license)) {
    return { expression: license }
  }
  return { license: { name: license } }
}
