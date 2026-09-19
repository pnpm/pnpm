import { describe, expect, it } from '@jest/globals'

import { classifyLicense } from '../src/license.js'

describe('classifyLicense', () => {
  it('returns canonical license.id values for SPDX license identifiers', () => {
    for (const [license, id] of [
      ['MIT', 'MIT'],
      ['mit', 'MIT'],
      [' MIT ', 'MIT'],
      ['GPL-2.0', 'GPL-2.0'],
      ['gpl-2.0', 'GPL-2.0'],
      ['GFDL-1.1-invariants-only', 'GFDL-1.1-invariants-only'],
      ['WTFPL', 'WTFPL'],
    ]) {
      expect(classifyLicense(license)).toEqual({ license: { id } })
    }
  })

  it('preserves SPDX 2.3 expressions', () => {
    for (const expression of [
      'MIT OR Apache-2.0',
      'mit OR apache-2.0',
      'MIT AND ISC',
      'GPL-2.0+',
      'LicenseRef-Proprietary',
      'DocumentRef-doc:LicenseRef-Custom',
      'GPL-2.0-only WITH Classpath-exception-2.0',
      'gpl-2.0-only WITH classpath-exception-2.0',
      '(MIT AND Apache-2.0) OR ISC',
    ]) {
      expect(classifyLicense(expression)).toEqual({ expression })
    }
  })

  it('preserves free-form license names', () => {
    for (const license of [
      'BDS-3-Clause',
      'UNLICENSED',
      'Proprietary License',
      'SEE LICENSE IN LICENSE.md',
      'LLVM-exception',
      'GFDL-1.1-invariants',
      'GFDL-1.1-invariants OR MIT',
      'NONE',
      'NOASSERTION',
      'NOASSERTION OR MIT',
      'MIT OR BDS-3-Clause',
      'MIT or Apache-2.0',
      'MIT WITH AdditionRef-Custom',
      'MIT WITH Unknown-exception',
      'MIT/Apache-2.0',
      'MIT OR',
    ]) {
      expect(classifyLicense(license)).toEqual({ license: { name: license } })
    }
  })
})
