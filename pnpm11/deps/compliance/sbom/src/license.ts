// Sub-path import to pull only the SPDX module — avoids dragging in the
// validation/serialize layers with optional native deps that break esbuild bundling.
import { fixupSpdxId, isSupportedSpdxId } from '@cyclonedx/cyclonedx-library/SPDX'
import parseSpdxExpression from 'spdx-expression-parse'

const SPDX_OPERATORS = ['AND', 'OR', 'WITH']

// Classifies a license string into the appropriate CycloneDX representation.
// license.id is an enum in the CycloneDX schema, so a value outside the SPDX
// license list — a typo, or npm's own UNLICENSED — has to arrive as a name or
// the whole document fails validation.
// Order matters: check ID first because "MIT" matches both isSupportedSpdxId
// and isSpdxExpression, but we prefer the more specific license.id form.
export function classifyLicense (license: string): { license: { id: string } } | { license: { name: string } } | { expression: string } {
  const trimmed = trimSpaces(license)
  const fixedId = fixupSpdxId(trimmed)
  // A trailing "+" makes the value an expression rather than an identifier, and
  // the CycloneDX list folds the SPDX exception identifiers into the same enum,
  // so isSpdxLicenseId keeps an exception such as LLVM-exception out of license.id.
  if (!trimmed.endsWith('+') && fixedId != null && isSupportedSpdxId(fixedId) && isSpdxLicenseId(fixedId)) {
    return { license: { id: fixedId } }
  }
  if (isSpdxExpression(license)) {
    return { expression: license }
  }
  return { license: { name: license } }
}

// Only the identifier lookup ignores the spaces around the value, so a value
// padded with a line break stays a name rather than becoming an expression no
// longer on one line.
function trimSpaces (value: string): string {
  let start = 0
  let end = value.length
  while (start < end && value[start] === ' ') start++
  while (end > start && value[end - 1] === ' ') end--
  return value.slice(start, end)
}

function isSpdxLicenseId (license: string): boolean {
  try {
    const parsed = parseSpdxExpression(license)
    return 'license' in parsed && parsed.exception == null
  } catch {
    return false
  }
}

// SPDX 2.3 annex D.2 matches license and exception identifiers case-insensitively
// and requires the operators to be uppercase, while spdx-expression-parse has it
// the other way around. That rewriting serves the decision alone, and the value
// validated here is the exact string the BOM will carry. The scanner skips only
// the ASCII space, so any other whitespace leaves the value a name.
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
