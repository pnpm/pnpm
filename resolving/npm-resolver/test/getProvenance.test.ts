import { type PackageInRegistry, type PackageMetaWithTime } from '@pnpm/registry.types'
import { getTrustEvidence, isProvenanceDowngraded } from '../src/getProvenance.js'

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

describe('isProvenanceDowngraded', () => {
  test('returns false when no versions have attestation', () => {
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
    expect(isProvenanceDowngraded(meta, '2.0.0')).toBe(false)
  })

  test('returns false for versions published before first attested version', () => {
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
    expect(isProvenanceDowngraded(meta, '1.0.0')).toBe(false)
  })

  test('returns true when downgrading from provenance to none', () => {
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
    expect(isProvenanceDowngraded(meta, '3.0.0')).toBe(true)
  })

  test('returns true when downgrading from trustedPublisher to provenance', () => {
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
    expect(isProvenanceDowngraded(meta, '3.0.0')).toBe(true)
  })

  test('returns true when downgrading from trustedPublisher to none', () => {
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
    expect(isProvenanceDowngraded(meta, '3.0.0')).toBe(true)
  })

  test('returns false when maintaining same provenance level', () => {
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
    expect(isProvenanceDowngraded(meta, '3.0.0')).toBe(false)
  })

  test('returns false when version time is missing', () => {
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
    expect(isProvenanceDowngraded(meta, '2.0.0')).toBeUndefined()
  })
})
