import { describe, expect, it } from '@jest/globals'
import { extractLicenseIds, matchLicenseAgainstPolicy } from '@pnpm/deps.compliance.license-checker'

describe('extractLicenseIds', () => {
  it('extracts a single license ID', () => {
    expect(extractLicenseIds('MIT')).toEqual(['MIT'])
  })

  it('extracts IDs from OR expression', () => {
    expect(extractLicenseIds('MIT OR Apache-2.0')).toEqual(['MIT', 'Apache-2.0'])
  })

  it('extracts IDs from AND expression', () => {
    expect(extractLicenseIds('MIT AND BSD-3-Clause')).toEqual(['MIT', 'BSD-3-Clause'])
  })

  it('extracts IDs from parenthesized expression', () => {
    expect(extractLicenseIds('(MIT OR Apache-2.0)')).toEqual(['MIT', 'Apache-2.0'])
  })

  it('extracts IDs from nested expression', () => {
    expect(extractLicenseIds('(MIT OR Apache-2.0) AND BSD-2-Clause')).toEqual(['MIT', 'Apache-2.0', 'BSD-2-Clause'])
  })

  it('extracts the base license ID from WITH expression (exception is not a separate ID)', () => {
    expect(extractLicenseIds('Apache-2.0 WITH LLVM-exception')).toEqual(['Apache-2.0'])
  })

  it('returns empty for empty string', () => {
    expect(extractLicenseIds('')).toEqual([])
  })
})

describe('matchLicenseAgainstPolicy', () => {
  describe('strict mode', () => {
    it('allows a license in the allowed list', () => {
      const result = matchLicenseAgainstPolicy('MIT', {
        allowed: new Set(['MIT', 'ISC']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('rejects a license not in the allowed list', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only', {
        allowed: new Set(['MIT', 'ISC']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('not-in-allowed-list')
    })

    it('rejects a license in the disallowed list', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('rejects unknown licenses', () => {
      const result = matchLicenseAgainstPolicy('Unknown', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('unknown-license')
    })

    it('rejects empty license string', () => {
      const result = matchLicenseAgainstPolicy('', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('unknown-license')
    })

    it('requires ALL parts of OR expression to be allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT OR GPL-3.0-only', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      // OR: either side passing is enough — MIT is allowed
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('rejects OR expression when no part is allowed', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only OR AGPL-3.0-only', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
    })

    it('allows OR expression when all parts are allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT OR Apache-2.0', {
        allowed: new Set(['MIT', 'Apache-2.0']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
    })

    it('requires ALL parts of AND expression to be allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT AND BSD-3-Clause', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('not-in-allowed-list')
    })

    it('allows AND expression when all parts are allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT AND BSD-3-Clause', {
        allowed: new Set(['MIT', 'BSD-3-Clause']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
    })

    it('rejects AND expression when any part is disallowed', () => {
      const result = matchLicenseAgainstPolicy('MIT AND GPL-3.0-only', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })
  })

  describe('WITH expressions', () => {
    it('checks the base license ID, not the exception', () => {
      const result = matchLicenseAgainstPolicy('Apache-2.0 WITH LLVM-exception', {
        allowed: new Set(['Apache-2.0']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('rejects when the base license is not allowed', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only WITH Classpath-exception-2.0', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
    })

    it('rejects when the base license is disallowed', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only WITH Classpath-exception-2.0', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('matches the full WITH literal in allowed list', () => {
      const result = matchLicenseAgainstPolicy('Apache-2.0 WITH LLVM-exception', {
        allowed: new Set(['Apache-2.0 WITH LLVM-exception']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('matches the full WITH literal in disallowed list', () => {
      const result = matchLicenseAgainstPolicy('Apache-2.0 WITH LLVM-exception', {
        disallowed: new Set(['Apache-2.0 WITH LLVM-exception']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })
  })

  describe('plus (or-later) expressions', () => {
    it('matches the base license ID in allowed list', () => {
      const result = matchLicenseAgainstPolicy('GPL-2.0+', {
        allowed: new Set(['GPL-2.0']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('matches the plus literal in allowed list', () => {
      const result = matchLicenseAgainstPolicy('GPL-2.0+', {
        allowed: new Set(['GPL-2.0+']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('matches the plus literal in disallowed list', () => {
      const result = matchLicenseAgainstPolicy('GPL-2.0+', {
        disallowed: new Set(['GPL-2.0+']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('disallows by base ID even for plus expressions', () => {
      const result = matchLicenseAgainstPolicy('GPL-2.0+', {
        disallowed: new Set(['GPL-2.0']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })
  })

  describe('nested expressions', () => {
    it('handles (MIT OR Apache-2.0) AND BSD-2-Clause', () => {
      const result = matchLicenseAgainstPolicy('(MIT OR Apache-2.0) AND BSD-2-Clause', {
        allowed: new Set(['MIT', 'BSD-2-Clause']),
        mode: 'strict',
      })
      // OR side: MIT is allowed. AND: BSD-2-Clause is allowed. Both pass.
      expect(result.allowed).toBe(true)
    })

    it('rejects nested expression when AND part is missing', () => {
      const result = matchLicenseAgainstPolicy('(MIT OR Apache-2.0) AND BSD-2-Clause', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      // OR side passes (MIT), but AND side fails (BSD-2-Clause not allowed)
      expect(result.allowed).toBe(false)
    })
  })

  describe('loose mode', () => {
    it('allows a license in the allowed list', () => {
      const result = matchLicenseAgainstPolicy('MIT', {
        allowed: new Set(['MIT']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('allows unlisted licenses by default', () => {
      const result = matchLicenseAgainstPolicy('BSD-3-Clause', {
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('allows unknown licenses', () => {
      const result = matchLicenseAgainstPolicy('Unknown', {
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('rejects disallowed licenses', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('allows OR expression if at least one part is allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT OR GPL-3.0-only', {
        allowed: new Set(['MIT']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('rejects OR expression only if ALL parts are disallowed', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only OR AGPL-3.0-only', {
        disallowed: new Set(['GPL-3.0-only', 'AGPL-3.0-only']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('allows OR expression if not all parts are disallowed', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only OR MIT', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
    })
  })

  describe('non-SPDX license strings', () => {
    it('allows a non-SPDX string that is in the allowed list (strict)', () => {
      const result = matchLicenseAgainstPolicy('Public Domain', {
        allowed: new Set(['Public Domain']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('explicitly-allowed')
    })

    it('rejects a non-SPDX string that is in the disallowed list (loose)', () => {
      const result = matchLicenseAgainstPolicy('Public Domain', {
        disallowed: new Set(['Public Domain']),
        mode: 'loose',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })

    it('rejects non-SPDX string not in allowed list (strict mode)', () => {
      const result = matchLicenseAgainstPolicy('Public Domain', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('not-in-allowed-list')
    })

    it('allows unmatched non-SPDX string in loose mode', () => {
      const result = matchLicenseAgainstPolicy('SEE LICENSE IN LICENSE', {
        mode: 'loose',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('treats unmatched non-SPDX string as unknown in strict mode with no allowed list', () => {
      const result = matchLicenseAgainstPolicy('SEE LICENSE IN LICENSE', {
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('unknown-license')
    })
  })
})
