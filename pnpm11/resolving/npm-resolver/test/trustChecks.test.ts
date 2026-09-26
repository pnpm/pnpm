import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { type LogBase, streamParser } from '@pnpm/logger'
import type { PackageInRegistry, PackageMetaWithTime } from '@pnpm/resolving.registry.types'

import { warnMissingTimeFieldOnce } from '../src/pickPackage.js'
import { failIfTrustDowngraded, getTrustEvidence } from '../src/trustChecks.js'

describe('getTrustEvidence', () => {
  test('returns undefined when _npmUser.trustedPublisher exists without provenance', () => {
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
    expect(getTrustEvidence(manifest)).toBeUndefined()
  })

  test('returns "trustedPublisher" when attestations.provenance also exists', () => {
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

  test('returns stagedPublish when approver exists', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      _npmUser: {
        name: 'test-approver',
        email: 'user@example.com',
        approver: {
          name: 'test-approver',
          email: 'user@example.com',
        },
      },
      dist: {
        shasum: 'abc123',
        tarball: 'https://registry.example.com/foo/-/foo-1.0.0.tgz',
      },
    }
    expect(getTrustEvidence(manifest)).toBe('stagedPublish')
  })

  test('returns stagedPublish when both approver and trustedPublisher exist', () => {
    const manifest: PackageInRegistry = {
      name: 'foo',
      version: '1.0.0',
      _npmUser: {
        name: 'test-approver',
        email: 'user@example.com',
        approver: {
          name: 'test-approver',
          email: 'user@example.com',
        },
        trustedPublisher: {
          id: 'test-provider',
          oidcConfigId: 'oidc:test-config-123',
        },
      },
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
    expect(getTrustEvidence(manifest)).toBe('stagedPublish')
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

  test('does not throw an error when only prerelease versions had provenance', () => {
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
        '2.0.0-0': {
          name: 'foo',
          version: '2.0.0-0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/foo/-/foo-2.0.0-0.tgz',
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
        '2.0.0-0': '2025-02-01T00:00:00.000Z',
        '3.0.0': '2025-03-01T00:00:00.000Z',
      },
    }
    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).not.toThrow()
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

  test('throws an error when downgrading from stagedPublish to trustedPublisher', () => {
    const meta: PackageMetaWithTime = {
      name: 'foo',
      'dist-tags': { latest: '2.0.0' },
      versions: {
        '1.0.0': {
          name: 'foo',
          version: '1.0.0',
          _npmUser: {
            name: 'test-approver',
            email: 'approver@example.com',
            approver: {
              name: 'test-approver',
              email: 'approver@example.com',
            },
          },
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
      failIfTrustDowngraded(meta, '2.0.0')
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
      failIfTrustDowngraded(meta, '3.0.0', { trustPolicyExclude: createPackageVersionPolicy(['foo@3.0.0']) })
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
      failIfTrustDowngraded(meta, '3.0.0', { trustPolicyExclude: createPackageVersionPolicy(['bar']) })
    }).not.toThrow()
  })

  test('does not fail with ERR_PNPM_MISSING_TIME when package@version is excluded and time field is missing', () => {
    const meta = {
      name: 'baz',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'baz',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/baz/-/baz-1.0.0.tgz',
          },
        },
      },
      // Note: no 'time' field
    }

    expect(() => {
      failIfTrustDowngraded(meta, '1.0.0', { trustPolicyExclude: createPackageVersionPolicy(['baz@1.0.0']) })
    }).not.toThrow()
  })

  test('does not fail with ERR_PNPM_MISSING_TIME when package name is excluded and time field is missing', () => {
    const meta = {
      name: 'qux',
      'dist-tags': { latest: '2.0.0' },
      versions: {
        '1.0.0': {
          name: 'qux',
          version: '1.0.0',
          dist: {
            shasum: 'abc123',
            tarball: 'https://registry.example.com/qux/-/qux-1.0.0.tgz',
          },
        },
        '2.0.0': {
          name: 'qux',
          version: '2.0.0',
          dist: {
            shasum: 'def456',
            tarball: 'https://registry.example.com/qux/-/qux-2.0.0.tgz',
          },
        },
      },
      // Note: no 'time' field
    }

    expect(() => {
      failIfTrustDowngraded(meta, '2.0.0', { trustPolicyExclude: createPackageVersionPolicy(['qux']) })
    }).not.toThrow()
  })
})

describe('failIfTrustDowngraded with trustPolicyIgnoreAfter', () => {
  test('allows downgrade when version is older than ignoreAfter threshold', () => {
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
      failIfTrustDowngraded(meta, '3.0.0', { trustPolicyIgnoreAfter: 60 * 24 * 30 }) // 30 days
    }).not.toThrow()

    expect(() => {
      failIfTrustDowngraded(meta, '3.0.0')
    }).toThrow('High-risk trust downgrade')
  })
})

describe('failIfTrustDowngraded with ignoreMissingTimeField', () => {
  const metaWithoutTime = {
    name: 'timeless',
    'dist-tags': { latest: '2.0.0' },
    versions: {
      '1.0.0': {
        name: 'timeless',
        version: '1.0.0',
        dist: {
          shasum: 'abc123',
          tarball: 'https://registry.example.com/timeless/-/timeless-1.0.0.tgz',
          attestations: {
            provenance: {
              predicateType: 'https://slsa.dev/provenance/v1',
            },
          },
        },
      },
      '2.0.0': {
        name: 'timeless',
        version: '2.0.0',
        dist: {
          shasum: 'def456',
          tarball: 'https://registry.example.com/timeless/-/timeless-2.0.0.tgz',
        },
      },
    },
    // Note: no 'time' field — a registry that strips it, or one whose
    // partial map `dropIncompletePublishTimes` normalized away.
  }

  test('fails with ERR_PNPM_MISSING_TIME when the flag is off', () => {
    expect(() => {
      failIfTrustDowngraded(metaWithoutTime, '2.0.0')
    }).toThrow('The metadata of timeless is missing the "time" field')
  })

  test('skips the check when the flag is on', () => {
    expect(() => {
      failIfTrustDowngraded(metaWithoutTime, '2.0.0', { ignoreMissingTimeField: true })
    }).not.toThrow()
  })

  test('still fails when the time map is present but omits the version', () => {
    // A packument that dates the versions it lists is saying it does not have
    // this one, so the gap stays a hard failure no matter how the flag is set.
    const meta: PackageMetaWithTime = {
      ...metaWithoutTime,
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
      },
    }

    expect(() => {
      failIfTrustDowngraded(meta, '2.0.0', { ignoreMissingTimeField: true })
    }).toThrow('Missing time for version 2.0.0 of timeless in metadata')
  })

  test('still reports a downgrade when the time map is complete', () => {
    const meta: PackageMetaWithTime = {
      ...metaWithoutTime,
      time: {
        '1.0.0': '2025-01-01T00:00:00.000Z',
        '2.0.0': '2025-02-01T00:00:00.000Z',
      },
    }

    expect(() => {
      failIfTrustDowngraded(meta, '2.0.0', { ignoreMissingTimeField: true })
    }).toThrow('High-risk trust downgrade')
  })
})

// Both time-dependent checks go dark on the same missing field, so the
// warning must name each one that was skipped rather than letting whichever
// ran first stand in for both.
describe('warnMissingTimeFieldOnce', () => {
  const collectedWarnings: string[] = []

  function collectWarnings (msg: LogBase & { message?: string }): void {
    if (msg.level === 'warn' && typeof msg.message === 'string') {
      collectedWarnings.push(msg.message)
    }
  }

  beforeEach(() => {
    collectedWarnings.length = 0
    streamParser.on('data', collectWarnings as (msg: LogBase) => void)
  })

  afterEach(() => {
    streamParser.removeListener('data', collectWarnings as (msg: LogBase) => void)
  })

  test('warns once per check for the same package, and no more', async () => {
    warnMissingTimeFieldOnce('dual-policy-pkg', 'minimumReleaseAge')
    warnMissingTimeFieldOnce('dual-policy-pkg', 'trustPolicy')
    warnMissingTimeFieldOnce('dual-policy-pkg', 'minimumReleaseAge')
    warnMissingTimeFieldOnce('dual-policy-pkg', 'trustPolicy')
    // The log stream delivers on its own turn of the event loop.
    await new Promise((resolve) => {
      setImmediate(resolve)
    })

    expect(collectedWarnings.filter((message) => message.includes('dual-policy-pkg'))).toStrictEqual([
      'The metadata of dual-policy-pkg is missing the "time" field; skipping the minimumReleaseAge check for this package.',
      'The metadata of dual-policy-pkg is missing the "time" field; skipping the trustPolicy check for this package.',
    ])
  })
})
