import { describe, expect, it } from '@jest/globals'
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
    namedRegistries,
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
