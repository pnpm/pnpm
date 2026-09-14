// Sub-path import to pull only the SPDX module — avoids dragging in the
// validation/serialize layers with optional native deps that break esbuild bundling.
import { fixupSpdxId, isSupportedSpdxId } from '@cyclonedx/cyclonedx-library/SPDX'
import parseSpdxExpression from 'spdx-expression-parse'

const SPDX_OPERATORS = ['AND', 'OR', 'WITH']

// Classifies a license string into the appropriate CycloneDX representation.
// Uses the CycloneDX library's own SPDX list rather than spdx-license-ids,
// since CycloneDX maintains its own subset of recognized IDs.
// Order matters: check ID first because "MIT" matches both isSupportedSpdxId
// and isSpdxExpression, but we prefer the more specific license.id form.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  const fixedId = fixupSpdxId(license)
  if (!license.endsWith('+') && fixedId != null && isSupportedSpdxId(fixedId) && isSpdxLicenseId(fixedId)) {
    return { license: { id: fixedId } }
  }
  if (isSpdxExpression(license)) {
    return { expression: license }
  }
  return { license: { name: license } }
}

function isSpdxLicenseId (license: string): boolean {
  try {
    const parsed = parseSpdxExpression(license)
    return 'license' in parsed && parsed.exception == null
  } catch {
    return false
  }
}

function isSpdxExpression (license: string): boolean {
  const normalized = normalizeSpdxExpressionIds(license)
  if (normalized == null) return false
  try {
    parseSpdxExpression(normalized)
    return true
  } catch {
    return false
  }
}

function normalizeSpdxExpressionIds (expression: string): string | undefined {
  const normalized: string[] = []
  let tokenStart = 0
  for (let index = 0; index < expression.length; index++) {
    if (isSpdxTokenChar(expression[index])) continue
    const normalizedToken = normalizeSpdxToken(expression.slice(tokenStart, index))
    if (normalizedToken == null) return undefined
    normalized.push(normalizedToken, expression[index])
    tokenStart = index + 1
  }
  const normalizedToken = normalizeSpdxToken(expression.slice(tokenStart))
  if (normalizedToken == null) return undefined
  normalized.push(normalizedToken)
  return normalized.join('')
}

function isSpdxTokenChar (char: string): boolean {
  return char >= 'A' && char <= 'Z' || char >= 'a' && char <= 'z' || char >= '0' && char <= '9' || char === '-' || char === '.'
}

function normalizeSpdxToken (token: string): string | undefined {
  if (SPDX_OPERATORS.some(operator => token.toUpperCase() === operator && token !== operator)) return undefined
  return fixupSpdxId(token) ?? token
}
