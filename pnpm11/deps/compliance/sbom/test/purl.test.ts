import { describe, expect, it } from '@jest/globals'
import { buildPurl } from '@pnpm/deps.compliance.sbom'

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

  it('should carry a repository_url qualifier for a named-registry package', () => {
    const registryUrl = 'https://npm.enterprise.example.com/'
    const purl = buildPurl({ name: 'foo', version: '1.0.0', registryUrl })

    const [base, qualifier] = purl.split('?repository_url=')
    expect(base).toBe('pkg:npm/foo@1.0.0')
    expect(decodeURIComponent(qualifier)).toBe(registryUrl)
  })

  it('should give the same name and version from two registries distinct purls', () => {
    const fromDefault = buildPurl({ name: 'foo', version: '1.0.0' })
    const fromNamed = buildPurl({
      name: 'foo',
      version: '1.0.0',
      registryUrl: 'https://npm.enterprise.example.com/',
    })
    // These two are different artifacts; collapsing them would drop one of
    // them from the SBOM entirely.
    expect(fromDefault).not.toBe(fromNamed)
  })

  it('should strip credentials from the repository_url qualifier', () => {
    const purl = buildPurl({
      name: 'foo',
      version: '1.0.0',
      registryUrl: 'https://some-user:some-token@npm.enterprise.example.com/team-a/?api_key=secret-value',
    })

    const qualifier = decodeURIComponent(purl.split('?repository_url=')[1])
    // An SBOM is meant to be published, so neither the userinfo nor a
    // token-bearing query string may travel with it. The path stays: two
    // registries can differ only by path.
    expect(qualifier).toBe('https://npm.enterprise.example.com/team-a/')
    for (const secret of ['some-user', 'some-token', 'secret-value']) {
      expect(purl).not.toContain(secret)
    }
  })
})
