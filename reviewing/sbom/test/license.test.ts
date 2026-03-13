import { describe, expect, it } from '@jest/globals'
import { classifyLicense } from '../src/license.js'

describe('classifyLicense', () => {
  it('should return license.id for a known SPDX identifier', () => {
    expect(classifyLicense('MIT')).toEqual({ license: { id: 'MIT' } })
    expect(classifyLicense('Apache-2.0')).toEqual({ license: { id: 'Apache-2.0' } })
    expect(classifyLicense('ISC')).toEqual({ license: { id: 'ISC' } })
    expect(classifyLicense('BSD-3-Clause')).toEqual({ license: { id: 'BSD-3-Clause' } })
  })

  it('should return expression for compound SPDX expressions with OR', () => {
    expect(classifyLicense('MIT OR Apache-2.0')).toEqual({ expression: 'MIT OR Apache-2.0' })
  })

  it('should return expression for compound SPDX expressions with AND', () => {
    expect(classifyLicense('MIT AND ISC')).toEqual({ expression: 'MIT AND ISC' })
  })

  it('should return expression for compound SPDX expressions with WITH', () => {
    expect(classifyLicense('Apache-2.0 WITH LLVM-exception')).toEqual({ expression: 'Apache-2.0 WITH LLVM-exception' })
  })

  it('should return license.name for non-SPDX license strings', () => {
    expect(classifyLicense('SEE LICENSE IN LICENSE.md')).toEqual({ license: { name: 'SEE LICENSE IN LICENSE.md' } })
    expect(classifyLicense('Custom License')).toEqual({ license: { name: 'Custom License' } })
  })

  it('should return license.name for unknown identifiers', () => {
    expect(classifyLicense('WTFPL')).toEqual({ license: { id: 'WTFPL' } })
  })
})
