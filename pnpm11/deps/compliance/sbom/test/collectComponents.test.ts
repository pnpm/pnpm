import { describe, expect, it } from '@jest/globals'
import { normalizeNamedRegistries } from '@pnpm/config.normalize-registries'
import { collectSbomComponents } from '@pnpm/deps.compliance.sbom'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'

const registries: Registries = { default: 'https://registry.npmjs.org/' }

/**
 * A project depending on `foo@1.0.0` from the default registry and on the
 * same name and version from a named registry.
 */
function lockfileWithBothRegistries (alias: string): LockfileObject {
  return {
    lockfileVersion: '9.1',
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          foo: '1.0.0',
          'foo-elsewhere': `foo@${alias}:1.0.0`,
        },
        specifiers: {
          foo: '^1.0.0',
          'foo-elsewhere': `${alias}:foo@^1.0.0`,
        },
      },
    },
    packages: {
      ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
      [`foo@${alias}:1.0.0` as DepPath]: { resolution: { integrity: 'sha512-BBBB' } },
    },
  } as unknown as LockfileObject
}

function collect (alias: string, namedRegistries?: Record<string, string>) {
  return collectSbomComponents({
    lockfile: lockfileWithBothRegistries(alias),
    rootName: 'root',
    rootVersion: '1.0.0',
    registries,
    namedRegistries: normalizeNamedRegistries(namedRegistries),
    lockfileDir: '/test',
    lockfileOnly: true,
  })
}

describe('collectSbomComponents with named registries', () => {
  it('should emit one component per registry rather than collapsing them', async () => {
    const { components } = await collect('work', { work: 'https://npm.enterprise.example.com/' })

    const fooPurls = components.map((c) => c.purl).filter((purl) => purl.startsWith('pkg:npm/foo@1.0.0'))
    expect(fooPurls).toHaveLength(2)
    expect(fooPurls.some((purl) => !purl.includes('repository_url'))).toBe(true)
    expect(fooPurls.some((purl) => purl.includes('repository_url'))).toBe(true)
  })

  it('should reject an alias that is not configured instead of dropping the component', async () => {
    // Without the alias the purl would fall back to the unqualified
    // `pkg:npm/foo@1.0.0`, collide with the default-registry component, and be
    // silently skipped by the dedupe shortcut — an artifact missing from a
    // compliance document.
    await expect(collect('work')).rejects.toThrow(
      expect.objectContaining({ code: 'ERR_PNPM_MISSING_NAMED_REGISTRY' })
    )
  })
})

describe('collectSbomComponents integrity', () => {
  /**
   * An SBOM checksum is read as an assurance that the artifact was
   * verified against it. pnpm never checks a git checkout against a hash,
   * so an `integrity` recorded on a `type: git` resolution must not be
   * republished as one.
   */
  it('should not publish an integrity recorded on a git resolution', async () => {
    const lockfile = {
      lockfileVersion: '9.1',
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: 'git+https://github.com/foo/bar.git#e63c09e460269b0c535e4c34debf69bb91d57b22' },
          specifiers: { foo: 'github:foo/bar' },
        },
      },
      packages: {
        ['foo@git+https://github.com/foo/bar.git#e63c09e460269b0c535e4c34debf69bb91d57b22' as DepPath]: {
          resolution: {
            type: 'git',
            repo: 'https://github.com/foo/bar.git',
            commit: 'e63c09e460269b0c535e4c34debf69bb91d57b22',
            integrity: 'sha512-AAAA',
          },
          version: '1.0.0',
        },
      },
    } as unknown as LockfileObject

    const { components } = await collectSbomComponents({
      lockfile,
      rootName: 'root',
      rootVersion: '1.0.0',
      registries,
      namedRegistries: normalizeNamedRegistries(undefined),
      lockfileDir: '/test',
      lockfileOnly: true,
    })

    const git = components.find((component) => component.name === 'foo')
    expect(git).toBeDefined()
    expect(git!.integrity).toBeUndefined()
  })

  /**
   * A `type: binary` runtime archive is checked against its integrity, so
   * that checksum is a real assurance and belongs in the SBOM.
   */
  it('should publish the integrity of a binary resolution', async () => {
    const lockfile = {
      lockfileVersion: '9.1',
      importers: {
        ['.' as ProjectId]: {
          dependencies: { node: '22.0.0' },
          specifiers: { node: '22.0.0' },
        },
      },
      packages: {
        ['node@22.0.0' as DepPath]: {
          resolution: {
            type: 'binary',
            archive: 'tarball',
            url: 'https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
            integrity: 'sha512-CCCC',
            bin: 'bin/node',
          },
          version: '22.0.0',
        },
      },
    } as unknown as LockfileObject

    const { components } = await collectSbomComponents({
      lockfile,
      rootName: 'root',
      rootVersion: '1.0.0',
      registries,
      namedRegistries: normalizeNamedRegistries(undefined),
      lockfileDir: '/test',
      lockfileOnly: true,
    })

    const binary = components.find((component) => component.name === 'node')
    expect(binary).toBeDefined()
    expect(binary!.integrity).toBe('sha512-CCCC')
  })
})

describe('verifiedIntegrity robustness', () => {
  /**
   * Lockfiles are untyped YAML, so `pnpm sbom` must survive a resolution
   * that is not the shape the types promise rather than crashing on it.
   */
  it.each([
    ['a non-object resolution', 'not-an-object'],
    ['a non-string integrity', { integrity: 12345 }],
    ['a non-string integrity on a binary resolution', { type: 'binary', integrity: 12345 }],
  ])('should omit the checksum for %s', async (_label, resolution) => {
    const lockfile = {
      lockfileVersion: '9.1',
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution },
      },
    } as unknown as LockfileObject

    const { components } = await collectSbomComponents({
      lockfile,
      rootName: 'root',
      rootVersion: '1.0.0',
      registries,
      namedRegistries: normalizeNamedRegistries(undefined),
      lockfileDir: '/test',
      lockfileOnly: true,
    })

    expect(components.find((component) => component.name === 'foo')?.integrity).toBeUndefined()
  })
})
