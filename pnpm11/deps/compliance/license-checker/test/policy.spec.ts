import { describe, expect, test } from '@jest/globals'

import { assertNoCompoundPolicyEntries, resolveLicensePolicy } from '../src/policy.js'

describe('resolveLicensePolicy', () => {
  test('null when no lists, overrides, or mode', () => {
    expect(resolveLicensePolicy(undefined)).toBeNull()
    expect(resolveLicensePolicy({})).toBeNull()
    expect(resolveLicensePolicy({ mode: 'loose' })).toBeNull() // nothing to check
  })

  test('null when mode is none even with lists', () => {
    expect(resolveLicensePolicy({ mode: 'none', disallowed: ['MIT'] })).toBeNull()
  })

  test('active with a disallowed list and no explicit mode, defaults to loose', () => {
    const p = resolveLicensePolicy({ disallowed: ['GPL-3.0-only'] })
    expect(p).not.toBeNull()
    expect(p!.mode).toBe('loose')
  })

  test('active with overrides only', () => {
    expect(resolveLicensePolicy({ overrides: { foo: true } })).not.toBeNull()
  })

  test('preserves explicit strict mode', () => {
    expect(resolveLicensePolicy({ mode: 'strict', allowed: ['MIT'] })!.mode).toBe('strict')
  })
})

// A hand-edited pnpm-workspace.yaml can put a compound (AND/OR) SPDX
// expression directly into `allowed`/`disallowed`, bypassing the rejection
// `pnpm licenses allow/disallow` applies at input time. This guard closes
// that gap at scan time.
describe('assertNoCompoundPolicyEntries', () => {
  test('throws when disallowed contains a compound OR expression', () => {
    expect(() => assertNoCompoundPolicyEntries({ disallowed: ['GPL-3.0-only OR GPL-2.0-only'] })).toThrow()
  })

  test('throws when allowed contains a compound AND expression', () => {
    expect(() => assertNoCompoundPolicyEntries({ allowed: ['MIT AND Apache-2.0'] })).toThrow()
  })

  test('thrown error uses the LICENSES_COMPOUND_EXPRESSION code (prefixed ERR_PNPM_)', () => {
    try {
      assertNoCompoundPolicyEntries({ disallowed: ['GPL-3.0-only OR GPL-2.0-only'] })
      throw new Error('expected assertNoCompoundPolicyEntries to throw')
    } catch (err) {
      expect((err as { code?: string }).code).toBe('ERR_PNPM_LICENSES_COMPOUND_EXPRESSION')
    }
  })

  test('thrown error names the offending entry and its field', () => {
    try {
      assertNoCompoundPolicyEntries({ disallowed: ['GPL-3.0-only OR GPL-2.0-only'] })
      throw new Error('expected assertNoCompoundPolicyEntries to throw')
    } catch (err) {
      expect((err as Error).message).toContain('GPL-3.0-only OR GPL-2.0-only')
      expect((err as Error).message).toContain('disallowed')
    }
  })

  test('thrown error names both fields when both allowed and disallowed have compounds', () => {
    try {
      assertNoCompoundPolicyEntries({
        allowed: ['MIT AND Apache-2.0'],
        disallowed: ['GPL-3.0-only OR GPL-2.0-only'],
      })
      throw new Error('expected assertNoCompoundPolicyEntries to throw')
    } catch (err) {
      const message = (err as Error).message
      expect(message).toContain('MIT AND Apache-2.0')
      expect(message).toContain('GPL-3.0-only OR GPL-2.0-only')
      expect(message).toContain('allowed')
      expect(message).toContain('disallowed')
    }
  })

  test('does not throw for a simple license id', () => {
    expect(() => assertNoCompoundPolicyEntries({ allowed: ['MIT'], disallowed: ['GPL-3.0-only'] })).not.toThrow()
  })

  test('does not throw for a plus (or-later) expression', () => {
    expect(() => assertNoCompoundPolicyEntries({ disallowed: ['GPL-2.0+'] })).not.toThrow()
  })

  test('does not throw for a WITH exception expression', () => {
    expect(() => assertNoCompoundPolicyEntries({ allowed: ['Apache-2.0 WITH LLVM-exception'] })).not.toThrow()
  })

  test('does not throw for a literal/non-SPDX string', () => {
    expect(() => assertNoCompoundPolicyEntries({ allowed: ['SEE LICENSE IN FILE'] })).not.toThrow()
  })

  test('does not throw for empty or undefined lists', () => {
    expect(() => assertNoCompoundPolicyEntries({})).not.toThrow()
    expect(() => assertNoCompoundPolicyEntries({ allowed: [], disallowed: [] })).not.toThrow()
  })
})
