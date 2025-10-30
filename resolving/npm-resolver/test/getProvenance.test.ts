import { type PackageInRegistry } from '@pnpm/registry.types'
import { getProvenance } from '../src/getProvenance.js'

describe('getProvenance', () => {
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
    expect(getProvenance(manifest)).toBe('trustedPublisher')
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
    expect(getProvenance(manifest)).toBe('trustedPublisher')
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
    expect(getProvenance(manifest)).toBe(true)
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
    expect(getProvenance(manifest)).toBeUndefined()
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
    expect(getProvenance(manifest)).toBeUndefined()
  })
})
