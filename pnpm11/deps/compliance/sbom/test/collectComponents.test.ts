import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { collectSbomComponents } from '@pnpm/deps.compliance.sbom'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath, ProjectId, RegistriesByScope } from '@pnpm/types'

const registriesByScope: RegistriesByScope = { default: 'https://registry.npmjs.org/' }

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

function collect (alias: string, registriesByPrefix?: Record<string, string>) {
  return collectSbomComponents({
    lockfile: lockfileWithBothRegistries(alias),
    rootName: 'root',
    rootVersion: '1.0.0',
    registriesByScope,
    registriesByPrefix: normalizeRegistriesByPrefix(registriesByPrefix),
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
      registriesByScope,
      registriesByPrefix: normalizeRegistriesByPrefix(undefined),
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
      registriesByScope,
      registriesByPrefix: normalizeRegistriesByPrefix(undefined),
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
      registriesByScope,
      registriesByPrefix: normalizeRegistriesByPrefix(undefined),
      lockfileDir: '/test',
      lockfileOnly: true,
    })

    const component = components.find((candidate) => candidate.name === 'foo')
    expect(component).toBeDefined()
    expect(component!.integrity).toBeUndefined()
  })
})

describe('collectSbomComponents with platform-incompatible packages', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(async () => {
    storeDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-sbom-installable-test-'))
    storeIndex = new StoreIndex(storeDir)

    const digest = 'abcd1234ef567890'
    const manifestPath = path.join(storeDir, 'files', digest.slice(0, 2), digest.slice(2))
    await fs.mkdir(path.dirname(manifestPath), { recursive: true })
    await fs.writeFile(manifestPath, JSON.stringify({
      name: 'installable',
      version: '1.0.0',
      license: 'MIT',
    }))

    const integrity = 'sha512-installable/1.0.0'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    // The store index key readPackageFileMap computes for a registry tarball.
    storeIndex.set(storeIndexKey(integrity, 'installable@1.0.0'), filesIndex)
  })

  afterAll(async () => {
    storeIndex.close()
    await fs.rm(storeDir, { recursive: true, force: true })
  })

  // Pinning the architecture keeps the verdict on the two Solaris packages the
  // same on every host the test runs on.
  const supportedArchitectures = { os: ['linux'], cpu: ['x64'], libc: ['glibc'] }

  function lockfileWithIncompatiblePackages (): LockfileObject {
    return {
      lockfileVersion: '9.0',
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            installable: '1.0.0',
            'never-installable-required': '1.0.0',
          },
          optionalDependencies: { 'never-installable': '1.0.0' },
          specifiers: {
            installable: '^1.0.0',
            'never-installable': '^1.0.0',
            'never-installable-required': '^1.0.0',
          },
        },
      },
      packages: {
        ['installable@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-installable/1.0.0' },
        },
        ['never-installable@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-never/1.0.0' },
          optional: true,
          os: ['solaris'],
        },
        ['never-installable-required@1.0.0' as DepPath]: {
          resolution: { integrity: 'sha512-never-required/1.0.0' },
          os: ['solaris'],
        },
      },
    } as unknown as LockfileObject
  }

  it('excludes platform-incompatible optional packages from the default SBOM', async () => {
    const { components, relationships } = await collectSbomComponents({
      lockfile: lockfileWithIncompatiblePackages(),
      rootName: 'root',
      rootVersion: '1.0.0',
      registriesByScope,
      lockfileDir: '/tmp/project',
      storeDir,
      supportedArchitectures,
      lockfileOnly: false,
    })

    const installable = components.find((c) => c.name === 'installable')
    expect(installable).toBeDefined()
    expect(installable?.license).toBe('MIT')

    // Optional packages for other platforms are not installed, so they have no
    // store metadata and are omitted from the SBOM — as is the edge pointing
    // at them, which would otherwise dangle.
    expect(components.find((c) => c.name === 'never-installable')).toBeUndefined()
    expect(relationships.some((r) => r.to === 'pkg:npm/never-installable@1.0.0')).toBe(false)

    // Non-optional packages are not filtered by this optional-platform logic:
    // the component and its relationship stay in the SBOM.
    const required = components.find((c) => c.name === 'never-installable-required')
    expect(required).toBeDefined()
    expect(
      relationships.some((r) => r.to === 'pkg:npm/never-installable-required@1.0.0')
    ).toBe(true)
  })

  it('keeps platform-incompatible optional packages in the --lockfile-only SBOM', async () => {
    const { components } = await collectSbomComponents({
      lockfile: lockfileWithIncompatiblePackages(),
      rootName: 'root',
      rootVersion: '1.0.0',
      registriesByScope,
      lockfileDir: '/tmp/project',
      storeDir,
      supportedArchitectures,
      lockfileOnly: true,
    })

    expect(components.find((c) => c.name === 'installable')).toBeDefined()
    // --lockfile-only describes the full lockfile graph, so an optional
    // package for another platform remains a component.
    expect(components.find((c) => c.name === 'never-installable')).toBeDefined()
  })
})
