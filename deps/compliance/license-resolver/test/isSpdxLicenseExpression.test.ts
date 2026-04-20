import { isSpdxLicenseExpression } from '@pnpm/deps.compliance.license-resolver'

describe('isSpdxLicenseExpression', () => {
  test.each([
    'MIT',
    'Apache-2.0',
    'ISC',
    'BSD-3-Clause',
    '0BSD',
    'LGPL-2.1-only',
  ])('accepts SPDX id: %s', (id) => {
    expect(isSpdxLicenseExpression(id)).toBe(true)
  })

  test('accepts a parenthesized OR expression of SPDX ids', () => {
    expect(isSpdxLicenseExpression('(MIT OR Apache-2.0)')).toBe(true)
  })

  test('accepts an unparenthesized OR expression', () => {
    expect(isSpdxLicenseExpression('MIT OR Apache-2.0')).toBe(true)
  })

  test.each([
    'Eclipse Public License 1.0',
    'Python Software Foundation License',
    'Apache 2.0',
    'BSD',
    'Apache2',
    'Unknown',
  ])('rejects non-SPDX name: %s', (name) => {
    expect(isSpdxLicenseExpression(name)).toBe(false)
  })

  test('rejects an OR expression containing a non-SPDX part', () => {
    expect(isSpdxLicenseExpression('(MIT OR Eclipse Public License 1.0)')).toBe(false)
  })

  test('rejects the empty string', () => {
    expect(isSpdxLicenseExpression('')).toBe(false)
  })
})
