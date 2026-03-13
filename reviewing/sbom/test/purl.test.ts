import { describe, expect, it } from '@jest/globals'
import { buildPurl } from '@pnpm/sbom'

describe('buildPurl', () => {
  it('should build a basic PURL for an unscoped package', () => {
    expect(buildPurl({ name: 'lodash', version: '4.17.21' }))
      .toBe('pkg:npm/lodash@4.17.21')
  })

  it('should build a PURL for a scoped package', () => {
    expect(buildPurl({ name: '@babel/core', version: '7.23.0' }))
      .toBe('pkg:npm/%40babel/core@7.23.0')
  })

  it('should include vcs_url for git deps', () => {
    const result = buildPurl({
      name: 'my-pkg',
      version: '1.0.0',
      nonSemverVersion: 'github.com/user/repo/abc123',
    })
    expect(result).toContain('pkg:npm/my-pkg@')
    expect(result).toContain('?vcs_url=')
    expect(result).toContain(encodeURIComponent('github.com/user/repo/abc123'))
  })

  it('should handle deeply scoped package names', () => {
    expect(buildPurl({ name: '@pnpm/lockfile.types', version: '1.0.0' }))
      .toBe('pkg:npm/%40pnpm/lockfile.types@1.0.0')
  })
})
