import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import { normalizeNamedRegistries } from '@pnpm/config.normalize-registries'
import { collectSbomComponents } from '@pnpm/deps.compliance.sbom'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
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

describe('collectSbomComponents with platform-incompatible packages', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(() => {
    storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-sbom-installable-test-'))
    storeIndex = new StoreIndex(storeDir)

    // The installable package is present in the store with a declared license.
    const digest = 'abcd1234ef567890'
    const manifestPath = path.join(storeDir, 'files', digest.slice(0, 2), digest.slice(2))
    fs.mkdirSync(path.dirname(manifestPath), { recursive: true })
    fs.writeFileSync(manifestPath, JSON.stringify({
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

  afterAll(() => {
    storeIndex.close()
    fs.rmSync(storeDir, { recursive: true, force: true })
  })

  // Both platform-incompatible packages are constrained to Solaris, which no
  // test matrix platform matches, so they are never installable anywhere.
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
      registries,
      lockfileDir: '/tmp/project',
      storeDir,
      lockfileOnly: false,
    })

    const installable = components.find((c) => c.name === 'installable')
    expect(installable).toBeDefined()
    expect(installable?.license).toBe('MIT')

    // Optional packages for other platforms are not installed, so they have no
    // store metadata and are omitted from the SBOM.
    expect(components.find((c) => c.name === 'never-installable')).toBeUndefined()

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
      registries,
      lockfileDir: '/tmp/project',
      storeDir,
      lockfileOnly: true,
    })

    expect(components.find((c) => c.name === 'installable')).toBeDefined()
    // --lockfile-only describes the full lockfile graph, so an optional
    // package for another platform remains a component.
    expect(components.find((c) => c.name === 'never-installable')).toBeDefined()
  })
})
