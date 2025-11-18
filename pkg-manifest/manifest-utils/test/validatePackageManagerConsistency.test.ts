import { validatePackageManagerConsistency } from '../src/validatePackageManagerConsistency.js'

describe('validatePackageManagerConsistency', () => {
  describe('Corepack compatibility', () => {
    test('allows when neither field exists', () => {
      const manifest = {}
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(0)
    })

    test('allows packageManager field alone', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(0)
    })

    test('allows devEngines.packageManager alone', () => {
      const manifest = {
        devEngines: {
          packageManager: { name: 'pnpm', version: '^9.0.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(0)
    })

    test('allows both when names match (no version conflict)', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: { name: 'pnpm', version: '9.5.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(0)
    })

    test('allows both when names match but warns about version conflict', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: { name: 'pnpm', version: '^9.0.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(1)
      expect(result.warnings[0]).toContain('"packageManager" field is set to')
      expect(result.warnings[0]).toContain('pnpm@9.5.0')
      expect(result.warnings[0]).toContain('^9.0.0')
    })

    test('rejects when names do not match (Corepack behavior)', () => {
      const manifest = {
        packageManager: 'yarn@4.0.0',
        devEngines: {
          packageManager: { name: 'pnpm', version: '^9.0.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(false)
      expect(result.error).toBeDefined()
      expect(result.error).toContain('does not match')
      expect(result.error).toContain('yarn@4.0.0')
      expect(result.error).toContain('pnpm')
    })

    test('allows array with matching packageManager name', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: [
            { name: 'pnpm', version: '^9.0.0' },
            { name: 'npm', version: '^10.0.0' },
          ],
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
      expect(result.warnings).toHaveLength(1) // Version mismatch warning
    })

    test('rejects array without matching packageManager name', () => {
      const manifest = {
        packageManager: 'bun@1.0.0',
        devEngines: {
          packageManager: [
            { name: 'pnpm', version: '^9.0.0' },
            { name: 'npm', version: '^10.0.0' },
          ],
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(false)
      expect(result.error).toContain('bun@1.0.0')
      expect(result.error).toContain('pnpm, npm')
    })
  })

  describe('Version conflict detection', () => {
    test('no warning when versions are identical', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: { name: 'pnpm', version: '9.5.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.warnings).toHaveLength(0)
    })

    test('warns when versions differ', () => {
      const manifest = {
        packageManager: 'npm@10.2.3',
        devEngines: {
          packageManager: { name: 'npm', version: '^10.0.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.warnings).toHaveLength(1)
      expect(result.warnings[0]).toContain('10.2.3')
      expect(result.warnings[0]).toContain('^10.0.0')
    })

    test('no warning when devEngines version is undefined', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: { name: 'pnpm' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.warnings).toHaveLength(0)
    })

    test('no warning when packageManager version is undefined', () => {
      const manifest = {
        packageManager: 'pnpm',
        devEngines: {
          packageManager: { name: 'pnpm', version: '^9.0.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.warnings).toHaveLength(0)
    })
  })

  describe('packageManager field parsing', () => {
    test('parses simple name without version', () => {
      const manifest = {
        packageManager: 'pnpm',
        devEngines: {
          packageManager: { name: 'pnpm' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
    })

    test('parses name with version', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0',
        devEngines: {
          packageManager: { name: 'pnpm', version: '9.5.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
    })

    test('parses name with version and hash', () => {
      const manifest = {
        packageManager: 'pnpm@9.5.0+sha256.abc123',
        devEngines: {
          packageManager: { name: 'pnpm', version: '9.5.0' },
        },
      }
      const result = validatePackageManagerConsistency(manifest)
      expect(result.isValid).toBe(true)
    })
  })
})
