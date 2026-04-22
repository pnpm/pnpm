import { describe, expect, test } from '@jest/globals'
import { parseLicenseFromManifest } from '@pnpm/deps.compliance.license-resolver'

describe('parseLicenseFromManifest', () => {
  test('reads a plain SPDX string from the modern `license` field', () => {
    expect(parseLicenseFromManifest({ license: 'MIT' })).toBe('MIT')
    expect(parseLicenseFromManifest({ license: 'Apache-2.0' })).toBe('Apache-2.0')
  })

  test('reads the `type` from the legacy `{type, url}` object form in `license`', () => {
    expect(parseLicenseFromManifest({
      license: { type: 'MIT', url: 'https://example.com/LICENSE' },
    })).toBe('MIT')
  })

  test('falls back to the legacy `licenses` array when `license` is absent', () => {
    // https://github.com/pnpm/pnpm/issues/11248 — busboy, streamsearch, etc.
    expect(parseLicenseFromManifest({
      licenses: [{ type: 'MIT', url: 'http://github.com/mscdex/busboy/raw/master/LICENSE' }],
    })).toBe('MIT')
  })

  // Pre-refactor license-scanner preferred `licenses` when both existed; we
  // intentionally flip that to match npm's modern precedence — see #11248.
  test('prefers modern `license` over legacy `licenses` when both are present', () => {
    expect(parseLicenseFromManifest({
      license: 'Apache-2.0',
      licenses: [{ type: 'MIT' }],
    })).toBe('Apache-2.0')

    // Still applies when `license` uses the legacy object form.
    expect(parseLicenseFromManifest({
      license: { type: 'Apache-2.0' },
      licenses: [{ type: 'MIT' }],
    })).toBe('Apache-2.0')
  })

  test('treats `license: ""` (empty string) as missing and falls through to `licenses`', () => {
    expect(parseLicenseFromManifest({
      license: '',
      licenses: [{ type: 'MIT' }],
    })).toBe('MIT')
  })

  test('ignores non-string license types (number, boolean, etc.)', () => {
    expect(parseLicenseFromManifest({ license: 42 })).toBeUndefined()
    expect(parseLicenseFromManifest({ license: true })).toBeUndefined()
    expect(parseLicenseFromManifest({ license: null })).toBeUndefined()
  })

  test('ignores legacy entries whose `type` / `name` are non-strings', () => {
    expect(parseLicenseFromManifest({
      licenses: [{ type: 42 }, { type: 'MIT' }],
    })).toBe('MIT')
  })

  // Matches pre-refactor license-scanner — no output change for packages that
  // happen to list the same type twice.
  test('does not deduplicate repeated legacy entries', () => {
    expect(parseLicenseFromManifest({
      licenses: [{ type: 'MIT' }, { type: 'MIT' }, { type: 'Apache-2.0' }],
    })).toBe('(MIT OR MIT OR Apache-2.0)')
  })

  test('joins multiple legacy entries as an SPDX OR expression wrapped in parens', () => {
    expect(parseLicenseFromManifest({
      licenses: [{ type: 'MIT' }, { type: 'Apache-2.0' }],
    })).toBe('(MIT OR Apache-2.0)')
  })

  test('falls back to `name` when `type` is missing on a legacy entry', () => {
    expect(parseLicenseFromManifest({
      licenses: [{ name: 'ISC' }],
    })).toBe('ISC')
  })

  test('accepts a single object in `licenses` (non-array legacy form)', () => {
    expect(parseLicenseFromManifest({
      licenses: { type: 'MIT' },
    })).toBe('MIT')
  })

  test('returns undefined when the manifest has no license information', () => {
    expect(parseLicenseFromManifest({})).toBeUndefined()
    expect(parseLicenseFromManifest({ license: '' })).toBeUndefined()
    expect(parseLicenseFromManifest({ licenses: [] })).toBeUndefined()
    expect(parseLicenseFromManifest({ licenses: [{ url: 'https://example.com' }] })).toBeUndefined()
  })
})
