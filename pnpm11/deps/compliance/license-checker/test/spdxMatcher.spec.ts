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

    it('allows OR expression if at least one part is allowed', () => {
      const result = matchLicenseAgainstPolicy('MIT OR GPL-3.0-only', {
        allowed: new Set(['MIT']),
        mode: 'strict',
      })
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
    it('does not treat an allowed base license as covering an exception variant', () => {
      // spdx-satisfies treats "Apache-2.0 WITH LLVM-exception" as a distinct
      // license term from plain "Apache-2.0" (per its own README: exceptions
      // must be listed explicitly, e.g. "Apache-2.0 WITH LLVM"). Allowing the
      // base license must not implicitly widen the policy to cover every
      // exception variant of it.
      const result = matchLicenseAgainstPolicy('Apache-2.0 WITH LLVM-exception', {
        allowed: new Set(['Apache-2.0']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('not-in-allowed-list')
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

    it('allows unmatched non-SPDX string in strict mode when no allowed list is configured', () => {
      // Strict + disallowed-only is a valid configuration: block listed
      // licenses, allow everything else (including unrecognized strings).
      const result = matchLicenseAgainstPolicy('SEE LICENSE IN LICENSE', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('allows unmatched non-SPDX string in strict mode with empty allowed list', () => {
      const result = matchLicenseAgainstPolicy('SEE LICENSE IN LICENSE', {
        allowed: new Set(),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })
  })

  describe('strict mode without allowed list', () => {
    it('allows unknown license when no allowed list is configured', () => {
      const result = matchLicenseAgainstPolicy('Unknown', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('allows empty license when no allowed list is configured', () => {
      const result = matchLicenseAgainstPolicy('', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(true)
      expect(result.reason).toBe('allowed-by-default')
    })

    it('still blocks explicitly disallowed licenses', () => {
      const result = matchLicenseAgainstPolicy('GPL-3.0-only', {
        disallowed: new Set(['GPL-3.0-only']),
        mode: 'strict',
      })
      expect(result.allowed).toBe(false)
      expect(result.reason).toBe('explicitly-disallowed')
    })
  })
})

describe('matchLicenseAgainstPolicy — hardened semantics', () => {
  test('case-insensitive: lowercase "mit" is caught by disallowed MIT', () => {
    const r = matchLicenseAgainstPolicy('mit', { disallowed: new Set(['MIT']), mode: 'loose' })
    expect(r.allowed).toBe(false)
    expect(r.reason).toBe('explicitly-disallowed')
  })

  test('plus-range: GPL-3.0-only satisfies allowed GPL-2.0+', () => {
    const r = matchLicenseAgainstPolicy('GPL-3.0-only', { allowed: new Set(['GPL-2.0+']), mode: 'strict' })
    expect(r.allowed).toBe(true)
  })

  test('OR-launder: disallowed GPL cannot be escaped via an unknown LicenseRef', () => {
    const r = matchLicenseAgainstPolicy('GPL-3.0-only OR LicenseRef-proprietary', {
      allowed: new Set(['MIT']),
      disallowed: new Set(['GPL-3.0-only']),
      mode: 'strict',
    })
    expect(r.allowed).toBe(false)
  })

  test('legitimate dual-license: MIT OR GPL-3.0 passes when MIT is allowed', () => {
    const r = matchLicenseAgainstPolicy('MIT OR GPL-3.0-only', {
      allowed: new Set(['MIT']),
      disallowed: new Set(['GPL-3.0-only']),
      mode: 'strict',
    })
    expect(r.allowed).toBe(true)
  })

  test('AND: MIT AND GPL-3.0 is rejected in strict mode when only MIT is allowed', () => {
    const r = matchLicenseAgainstPolicy('MIT AND GPL-3.0-only', { allowed: new Set(['MIT']), mode: 'strict' })
    expect(r.allowed).toBe(false)
  })

  test('unparseable license is not silently allowed against a disallow list of a different case', () => {
    const r = matchLicenseAgainstPolicy('Apache 2.0', { disallowed: new Set(['Apache-2.0']), mode: 'loose' })
    // "Apache 2.0" (space) is not the SPDX id; it is treated as unknown, not a bypass of Apache-2.0
    expect(r.reason).not.toBe('explicitly-disallowed')
    expect(r.allowed).toBe(true) // loose + no allowed list ⇒ allowed-by-default, documented
  })
})

describe('matchLicenseAgainstPolicy — regression fixes', () => {
  test('does not throw when an allowed entry is not valid SPDX', () => {
    expect(() => matchLicenseAgainstPolicy('MIT', { allowed: new Set(['Apache 2.0', 'MIT']), mode: 'strict' })).not.toThrow()
    expect(matchLicenseAgainstPolicy('MIT', { allowed: new Set(['Apache 2.0', 'MIT']), mode: 'strict' }).allowed).toBe(true)
    const r = matchLicenseAgainstPolicy('GPL-3.0-only', { allowed: new Set(['Apache 2.0']), mode: 'strict' })
    expect(r.allowed).toBe(false)
    expect(r.reason).toBe('not-in-allowed-list')
  })

  test('a standalone LicenseRef is not falsely reported as disallowed', () => {
    const r = matchLicenseAgainstPolicy('LicenseRef-proprietary', { disallowed: new Set(['GPL-3.0-only']), mode: 'loose' })
    expect(r.allowed).toBe(true)
    expect(r.reason).not.toBe('explicitly-disallowed')
  })

  test('LicenseRef in an AND with a non-disallowed license is not blocked', () => {
    const r = matchLicenseAgainstPolicy('MIT AND LicenseRef-x', { disallowed: new Set(['GPL-3.0-only']), mode: 'loose' })
    expect(r.allowed).toBe(true)
  })

  test('a non-disallowed real license is a valid OR escape from a disallowed sibling', () => {
    // GPL OR BSD with only GPL disallowed: BSD is a real escape, disallow passes
    const r = matchLicenseAgainstPolicy('GPL-3.0-only OR BSD-3-Clause', { disallowed: new Set(['GPL-3.0-only']), mode: 'loose' })
    expect(r.allowed).toBe(true)
  })
})
