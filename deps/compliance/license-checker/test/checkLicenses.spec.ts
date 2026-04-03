import { describe, expect, it } from '@jest/globals'
import { checkLicenseCompliance, type LicensePackageInfo } from '@pnpm/deps.compliance.license-checker'

function pkg (overrides: Partial<LicensePackageInfo> = {}): LicensePackageInfo {
  return {
    name: 'test-pkg',
    version: '1.0.0',
    license: 'MIT',
    belongsTo: 'dependencies',
    ...overrides,
  }
}

describe('checkLicenseCompliance', () => {
  it('returns empty result for mode none', () => {
    const result = checkLicenseCompliance(
      [pkg()],
      { mode: 'none' }
    )
    expect(result.violations).toEqual([])
    expect(result.warnings).toEqual([])
    expect(result.checkedCount).toBe(0)
  })

  describe('strict mode', () => {
    it('passes when all licenses are in allowed list', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'MIT' }), pkg({ name: 'pkg-b', license: 'ISC' })],
        { mode: 'strict', allowed: ['MIT', 'ISC'] }
      )
      expect(result.violations).toEqual([])
      expect(result.checkedCount).toBe(2)
    })

    it('reports violation when license is not in allowed list', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'GPL-3.0-only' })],
        { mode: 'strict', allowed: ['MIT'] }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.violations[0].packageName).toBe('test-pkg')
      expect(result.violations[0].license).toBe('GPL-3.0-only')
    })

    it('reports violation when license is in disallowed list', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'GPL-3.0-only' })],
        { mode: 'strict', disallowed: ['GPL-3.0-only'] }
      )
      expect(result.violations).toHaveLength(1)
    })

    it('reports violation for unknown license', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'Unknown' })],
        { mode: 'strict', allowed: ['MIT'] }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.violations[0].reason).toContain('unknown license')
    })
  })

  describe('loose mode', () => {
    it('passes unlisted licenses without violation', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'BSD-3-Clause' })],
        { mode: 'loose', allowed: ['MIT'] }
      )
      expect(result.violations).toEqual([])
    })

    it('reports violation for explicitly disallowed license even in loose mode', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'GPL-3.0-only' })],
        { mode: 'loose', disallowed: ['GPL-3.0-only'] }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.warnings).toEqual([])
    })
  })

  describe('overrides', () => {
    it('skips package with override true', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'GPL-3.0-only' })],
        { mode: 'strict', allowed: ['MIT'], overrides: { 'test-pkg': true } }
      )
      expect(result.violations).toEqual([])
    })

    it('uses override license string for checking', () => {
      const result = checkLicenseCompliance(
        [pkg({ license: 'Unknown' })],
        { mode: 'strict', allowed: ['MIT'], overrides: { 'test-pkg': 'MIT' } }
      )
      expect(result.violations).toEqual([])
    })

    it('applies version-specific override', () => {
      const result = checkLicenseCompliance(
        [
          pkg({ name: 'test-pkg', version: '1.0.0', license: 'GPL-3.0-only' }),
          pkg({ name: 'test-pkg', version: '2.0.0', license: 'GPL-3.0-only' }),
        ],
        { mode: 'strict', allowed: ['MIT'], overrides: { 'test-pkg@1.0.0': true } }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.violations[0].packageVersion).toBe('2.0.0')
    })

    it('prefers version-specific override over package-level override', () => {
      const result = checkLicenseCompliance(
        [pkg({ version: '1.0.0', license: 'GPL-3.0-only' })],
        {
          mode: 'strict',
          allowed: ['MIT'],
          overrides: {
            'test-pkg': 'Apache-2.0',
            'test-pkg@1.0.0': true,
          },
        }
      )
      expect(result.violations).toEqual([])
    })
  })

  describe('environment filtering', () => {
    it('checks only prod deps when environment is prod', () => {
      const result = checkLicenseCompliance(
        [
          pkg({ belongsTo: 'dependencies', license: 'GPL-3.0-only' }),
          pkg({ name: 'dev-pkg', belongsTo: 'devDependencies', license: 'GPL-3.0-only' }),
        ],
        { mode: 'strict', disallowed: ['GPL-3.0-only'], environment: 'prod' }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.violations[0].packageName).toBe('test-pkg')
    })

    it('checks only dev deps when environment is dev', () => {
      const result = checkLicenseCompliance(
        [
          pkg({ belongsTo: 'dependencies', license: 'GPL-3.0-only' }),
          pkg({ name: 'dev-pkg', belongsTo: 'devDependencies', license: 'GPL-3.0-only' }),
        ],
        { mode: 'strict', disallowed: ['GPL-3.0-only'], environment: 'dev' }
      )
      expect(result.violations).toHaveLength(1)
      expect(result.violations[0].packageName).toBe('dev-pkg')
    })

    it('checks all deps when environment is all', () => {
      const result = checkLicenseCompliance(
        [
          pkg({ belongsTo: 'dependencies', license: 'GPL-3.0-only' }),
          pkg({ name: 'dev-pkg', belongsTo: 'devDependencies', license: 'GPL-3.0-only' }),
        ],
        { mode: 'strict', disallowed: ['GPL-3.0-only'], environment: 'all' }
      )
      expect(result.violations).toHaveLength(2)
    })

    it('defaults to all when environment is not specified', () => {
      const result = checkLicenseCompliance(
        [
          pkg({ belongsTo: 'dependencies', license: 'GPL-3.0-only' }),
          pkg({ name: 'dev-pkg', belongsTo: 'devDependencies', license: 'GPL-3.0-only' }),
        ],
        { mode: 'strict', disallowed: ['GPL-3.0-only'] }
      )
      expect(result.violations).toHaveLength(2)
    })
  })
})
