import { describe, expect, it } from '@jest/globals'

import type { AquaRegistryPackage } from '../src/registry.js'
import { findMatchingOverride } from '../src/registry.js'

describe('findMatchingOverride', () => {
  const pkg: AquaRegistryPackage = {
    type: 'github_release',
    repo_owner: 'BurntSushi',
    repo_name: 'ripgrep',
    version_constraint: 'false',
    version_overrides: [
      {
        version_constraint: 'semver("<= 0.1.0")',
        asset: 'old-{{.Version}}.tar.gz',
        format: 'tar.gz',
      },
      {
        version_constraint: 'Version == "1.0.0-beta"',
        asset: 'beta-{{.Version}}.tar.gz',
        format: 'tar.gz',
      },
      {
        version_constraint: 'semver("<= 13.0.0")',
        asset: 'mid-{{.Version}}.tar.gz',
        format: 'tar.gz',
      },
      {
        version_constraint: 'true',
        asset: 'latest-{{.Version}}.tar.gz',
        format: 'tar.gz',
      },
    ],
  }

  it('matches the catch-all override for recent versions', () => {
    const result = findMatchingOverride(pkg, '14.1.1')
    expect(result.asset).toBe('latest-{{.Version}}.tar.gz')
  })

  it('matches semver range for older versions', () => {
    const result = findMatchingOverride(pkg, '0.0.5')
    expect(result.asset).toBe('old-{{.Version}}.tar.gz')
  })

  it('matches mid-range versions', () => {
    const result = findMatchingOverride(pkg, '12.0.0')
    expect(result.asset).toBe('mid-{{.Version}}.tar.gz')
  })

  it('matches exact version constraints', () => {
    const result = findMatchingOverride(pkg, '1.0.0-beta')
    expect(result.asset).toBe('beta-{{.Version}}.tar.gz')
  })

  it('handles v-prefixed versions', () => {
    const result = findMatchingOverride(pkg, 'v14.1.1')
    expect(result.asset).toBe('latest-{{.Version}}.tar.gz')
  })
})
