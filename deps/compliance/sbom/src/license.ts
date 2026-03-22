// Sub-path import to pull only the SPDX module — avoids dragging in the
// validation/serialize layers with optional native deps that break esbuild bundling.
import { fixupSpdxId, isSupportedSpdxId } from '@cyclonedx/cyclonedx-library/SPDX'

// Classifies a license string into the appropriate CycloneDX representation.
// Uses the CycloneDX library's own SPDX list rather than spdx-license-ids,
// since CycloneDX maintains its own subset of recognized IDs.
// Order matters: check ID first because "MIT" matches both isSupportedSpdxId
// and isSpdxExpression, but we prefer the more specific license.id form.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  const fixedId = fixupSpdxId(license)
  if (fixedId != null && isSupportedSpdxId(fixedId)) {
    return { license: { id: fixedId } }
  }
  if (isSpdxExpression(license)) {
    return { expression: license }
  }
  return { license: { name: license } }
}

// Checks if a string looks like an SPDX license expression (compound with operators).
function isSpdxExpression (license: string): boolean {
  return /\b(?:AND|OR|WITH)\b/.test(license)
}
