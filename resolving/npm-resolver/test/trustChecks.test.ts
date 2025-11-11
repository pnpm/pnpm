import { type PackageInRegistry, type PackageMetaWithTime } from '@pnpm/registry.types'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { getTrustEvidence, failIfTrustDowngraded } from '../src/trustChecks.js'

describe('getTrustEvidence', () => {
  test('returns "trustedPublisher" when _npmUser.trustedPublisher exists', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      _npmUser: {
        name: 'test-publisher',
        email: 'publisher@example.com',
        trustedPublisher: {
          id: 'test-provider',
          oidcConfigId: 'oidc:test-config-123',
        },
      },
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
      },
    }
    expect(getTrustEvidence(manifest)).toBe('trustedPublisher')
  })

  test('returns "trustedPublisher" even when attestations.provenance exists', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      _npmUser: {
        name: 'test-publisher',
        email: 'publisher@example.com',
        trustedPublisher: {
          id: 'test-provider',
          oidcConfigId: 'oidc:test-config-123',
        },
      },
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
        attestations: {
          provenance: {
            predicateType: 'https://slsa.dev/provenance/v1',
          },
        },
      },
    }
    expect(getTrustEvidence(manifest)).toBe('trustedPublisher')
  })

  test('returns true when provenance exists', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
        attestations: {
          provenance: {
            predicateType: 'https://slsa.dev/provenance/v1',
          },
        },
      },
    }
    expect(getTrustEvidence(manifest)).toBe('provenance')
  })

  test('returns undefined when provenance and attestations are undefined', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
      },
    }
    expect(getTrustEvidence(manifest)).toBeUndefined()
  })

  test('returns undefined when _npmUser exists but trustedPublisher is undefined', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      _npmUser: {
        name: 'test-user',
        email: 'user@example.com',
      },
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
      },
    }
    expect(getTrustEvidence(manifest)).toBeUndefined()
  })
})

describe('failIfTrustDowngraded', () => {
  test('succeeds when no versions have attestation', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '2.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '2.0.0')
    }).not.toThrow()
  })

  test('succeeds for version published before first attested version', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '2.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '1.0.0')
    }).not.toThrow()
  })

  test('throws an error when downgrading from provenance to none', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
          },
        },
        '3.0.0': {
          name: 'foo',
          version: '3.0.0',
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/foo/-/foo-3.0.0.tgz',
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).toThrow('High-risk trust downgrade')
  })

  test('throws an error when downgrading from trustedPublisher to provenance', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
          },
        },
        '3.0.0': {
          name: 'foo',
          version: '3.0.0',
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/foo/-/foo-3.0.0.tgz',
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).toThrow('High-risk trust downgrade')
  })

  test('throws an error when downgrading from trustedPublisher to none', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
          },
        },
        '3.0.0': {
          name: 'foo',
          version: '3.0.0',
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/foo/-/foo-3.0.0.tgz',
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).toThrow('High-risk trust downgrade')
  })

  test('succeeds when maintaining same trust level', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
          },
        },
        '3.0.0': {
          name: 'foo',
          version: '3.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/foo/-/foo-3.0.0.tgz',
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).not.toThrow()
  })

  test('throws an error when version time is missing', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '2.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
          },
        },
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
          },
        },
      },
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '2.0.0')
    }).toThrow('Missing time')
  })
})

describe('failIfTrustDowngraded with trustPolicyExclude', () => {
  test('allows downgrade when package@version is in exclude list', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '2.0.0': {
          name: 'foo',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0.tgz',
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
          },
        },
        '3.0.0': {
          name: 'foo',
          version: '3.0.0',
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/foo/-/foo-3.0.0.tgz',
          },
        },
      },
      time: {
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }

    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0', createPackageVersionPolicy(['foo@3.0.0']))
    }).not.toThrow()

    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).toThrow('High-risk trust downgrade')
  })

  test('allows downgrade when package name is in exclude list (all versions)', () => {
    const meta: PackageMetaWithTime = {
      name: 'bar',
      'dist-tags': { latest: '3.0.0' },
      versions: {
        '2.0.0': {
          name: 'bar',
          version: '2.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/bar/-/bar-2.0.0.tgz',
          },
        },
        '3.0.0': {
          name: 'bar',
          version: '3.0.0',
          dist: {
            shasum: 'ghi789',
            tarball: 'https://registry.example.com/bar/-/bar-3.0.0.tgz',
          },
        },
      },
      time: {
        '2.0.0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }

    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0', createPackageVersionPolicy(['bar']))
    }).not.toThrow()
  })
})
