import { describe, expect, test } from '@jest/globals'

import { resolveLicensePolicy } from '../src/policy.js'

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
