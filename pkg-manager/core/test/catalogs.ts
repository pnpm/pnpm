import { type Catalogs } from '@pnpm/catalogs.types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { createPeersDirSuffix } from '@pnpm/dependency-path'
import { type Lockfile } from '@pnpm/lockfile-types'
import { type ProjectId, type ProjectManifest } from '@pnpm/types'
import { preparePackages } from '@pnpm/prepare'
import { type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/core'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/workspace.find-packages'
import readYamlFile from 'read-yaml-file'
import path from 'path'
import { testDefaults } from './utils'

/**
 * A utility to make writing catalog tests easier by reducing boilerplate.
 */
class CatalogTestsController {
  private readonly workspaceDir: string
  private projects: Record<ProjectId, ProjectManifest>
  private catalogs: Catalogs = {}

  constructor (pkgs: Array<{ location: string, package: ProjectManifest }>) {
    preparePackages(pkgs)
    this.workspaceDir = process.cwd()

    this.projects = {}
    for (const { location, package: manifest } of pkgs) {
      this.projects[location as ProjectId] = manifest
    }
  }

  setCatalogs (catalogs: Catalogs) {
    this.catalogs = catalogs
  }

  async install (opts?: { filter?: readonly string[] }) {
    const importers: MutatedProject[] = Object.entries(this.projects)
      .filter(([id]) => opts?.filter?.includes(id) ?? true)
      .map(([id, manifest]) => ({
        mutation: 'install',
        id,
        manifest,
        rootDir: path.join(this.workspaceDir, id),
      }))

    const mutateModulesAllProjects: ProjectOptions[] = Object.entries(this.projects)
      .map(([id, manifest]) => ({
        buildIndex: 0,
        manifest,
        dir: path.join(this.workspaceDir, id),
        rootDir: path.join(this.workspaceDir, id),
      }))

    await mutateModules(importers, testDefaults({
      allProjects: mutateModulesAllProjects,
      lockfileOnly: true,
      catalogs: this.catalogs,
      workspacePackages: arrayOfWorkspacePackagesToMap(mutateModulesAllProjects),
    }))
  }

  async lockfile (): Promise<Lockfile> {
    return readYamlFile(path.join(this.workspaceDir, WANTED_LOCKFILE))
  }

  updateProjectManifest (location: ProjectId, manifest: ProjectManifest): void {
    this.projects = {
      ...this.projects,
      [location]: manifest,
    }
  }
}

test('installing with "catalog:" should work', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          'is-positive': 'catalog:',
        },
      },
    },
    // Empty second project to create a multi-package workspace.
    {
      location: 'packages/project2',
      package: {},
    },
  ])

  ctrl.setCatalogs({
    default: { 'is-positive': '1.0.0' },
  })
  await ctrl.install()
  const lockfile = await ctrl.lockfile()

  expect(lockfile.importers['packages/project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })
})

test('importer to importer dependency with "catalog:" should work', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        name: 'project1',
        dependencies: {
          project2: 'workspace:*',
        },
      },
    },
    {
      location: 'packages/project2',
      package: {
        name: 'project2',
        dependencies: {
          'is-positive': 'catalog:',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: { 'is-positive': '1.0.0' },
  })

  await ctrl.install()
  const lockfile = await ctrl.lockfile()

  expect(lockfile.importers['packages/project2' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })
})

test('importer with different peers uses correct peer', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          '@pnpm.e2e/has-foo100-peer': 'catalog:',
          // Define a peer with an exact version to ensure the dep above uses
          // this peer.
          '@pnpm.e2e/foo': '100.0.0',
        },
      },
    },
    {
      location: 'packages/project2',
      package: {
        dependencies: {
          '@pnpm.e2e/has-foo100-peer': 'catalog:',
          // Note that this peer is intentionally different than the one above
          // for project 1. (100.1.0 instead of 100.0.0).
          //
          // We want to ensure project2 resolves to the same catalog version for
          // @pnpm.e2e/has-foo100-peer, but uses a different peers suffix.
          //
          // Catalogs allow versions to be reused, but this test ensures we
          // don't reuse versions too aggressively.
          '@pnpm.e2e/foo': '100.1.0',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: {
      '@pnpm.e2e/has-foo100-peer': '^1.0.0',
    },
  })

  await ctrl.install()
  const lockfile = await ctrl.lockfile()

  expect(lockfile.importers['packages/project1' as ProjectId]?.dependencies?.['@pnpm.e2e/has-foo100-peer']).toEqual({
    specifier: 'catalog:',
    version: `1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/foo', version: '100.0.0' }])}`,
  })
  expect(lockfile.importers['packages/project2' as ProjectId]?.dependencies?.['@pnpm.e2e/has-foo100-peer']).toEqual({
    specifier: 'catalog:',
    //              This version is intentionally different from the one above    ꜜ
    version: `1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/foo', version: '100.1.0' }])}`,
  })
})

test('lockfile contains catalog snapshots', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          'is-positive': 'catalog:',
        },
      },
    },
    {
      location: 'packages/project2',
      package: {
        dependencies: {
          'is-negative': 'catalog:',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
  })

  await ctrl.install()
  const lockfile = await ctrl.lockfile()

  expect(lockfile.catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },
    },
  })
})

test('lockfile is updated if catalog config changes', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          'is-positive': 'catalog:',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: { 'is-positive': '=1.0.0' },
  })
  await ctrl.install()

  expect((await ctrl.lockfile()).importers['packages/project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })

  ctrl.setCatalogs({
    default: {
      'is-positive': '=3.1.0',
    },
  })
  await ctrl.install()

  expect((await ctrl.lockfile()).importers['packages/project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '3.1.0',
      },
    },
  })
})

test('lockfile catalog snapshots retain existing entries on --filter', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          'is-negative': 'catalog:',
        },
      },
    },
    {
      location: 'packages/project2',
      package: {
        dependencies: {
          'is-positive': 'catalog:',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
  })

  await ctrl.install()

  expect((await ctrl.lockfile()).catalogs).toStrictEqual({
    default: {
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
    },
  })

  // Update catalog definitions so pnpm triggers a rerun.
  ctrl.setCatalogs({
    default: {
      'is-positive': '=3.1.0',
      'is-negative': '^1.0.0',
    },
  })
  await ctrl.install({ filter: ['packages/project2'] })

  expect((await ctrl.lockfile()).catalogs).toStrictEqual({
    default: {
      // The is-negative snapshot should be carried from the previous install,
      // despite the current filtered install not using it.
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },

      'is-positive': { specifier: '=3.1.0', version: '3.1.0' },
    },
  })
})

// If a catalog specifier was used in one or more package.json files and all
// usages were removed later, we should remove the catalog snapshot from
// pnpm-lock.yaml. This should happen even if the dependency is still defined in
// a catalog under pnpm-workspace.yaml.
//
// Note that this behavior may not be desirable in all cases. If someone removes
// the last usage of a catalog entry, and another person adds it back later,
// that dependency will be re-resolved to a newer version. This is probably
// desirable most of the time, but there could be a good argument to cache the
// older unused resolution. For now we'll remove the unused entries since that's
// what would happen anyway if catalogs aren't used.
test('lockfile catalog snapshots should remove unused entries', async () => {
  const ctrl = new CatalogTestsController([
    {
      location: 'packages/project1',
      package: {
        dependencies: {
          'is-negative': 'catalog:',
          'is-positive': 'catalog:',
        },
      },
    },
  ])

  ctrl.setCatalogs({
    default: {
      'is-negative': '=1.0.0',
      'is-positive': '=1.0.0',
    },
  })

  {
    await ctrl.install()
    const lockfile = await ctrl.lockfile()
    expect(lockfile.importers['packages/project1' as ProjectId]?.dependencies).toEqual({
      'is-negative': { specifier: 'catalog:', version: '1.0.0' },
      'is-positive': { specifier: 'catalog:', version: '1.0.0' },
    })
    expect(lockfile.catalogs?.default).toStrictEqual({
      'is-negative': { specifier: '=1.0.0', version: '1.0.0' },
      'is-positive': { specifier: '=1.0.0', version: '1.0.0' },
    })
  }

  // Update package.json to no longer depend on is-positive.
  ctrl.updateProjectManifest('packages/project1' as ProjectId, {
    dependencies: {
      'is-negative': 'catalog:',
    },
  })
  await ctrl.install()

  {
    const lockfile = await ctrl.lockfile()
    expect(lockfile.importers['packages/project1' as ProjectId]?.dependencies).toEqual({
      'is-negative': { specifier: 'catalog:', version: '1.0.0' },
    })
    // Only "is-negative" should be in the catalogs section of the lockfile
    // since all packages in the workspace no longer use is-positive. Note that
    // this should be the case even if pnpm-workspace.yaml still has
    // "is-positive" configured.
    expect(lockfile.catalogs?.default).toStrictEqual({
      'is-negative': { specifier: '=1.0.0', version: '1.0.0' },
    })
  }
})
