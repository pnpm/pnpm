import { createPeersDirSuffix } from '@pnpm/dependency-path'
import { type ProjectRootDir, type ProjectId, type ProjectManifest } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { type MutatedProject, mutateModules, type ProjectOptions, type MutateModulesOptions } from '@pnpm/core'
import path from 'path'
import { testDefaults } from './utils'

function preparePackagesAndReturnObjects (manifests: Array<ProjectManifest & Required<Pick<ProjectManifest, 'name'>>>) {
  const project = prepareEmpty()
  const projects: Record<ProjectId, ProjectManifest> = {}
  for (const manifest of manifests) {
    projects[manifest.name as ProjectId] = manifest
  }
  const allProjects: ProjectOptions[] = Object.entries(projects)
    .map(([id, manifest]) => ({
      buildIndex: 0,
      manifest,
      rootDir: path.resolve(id) as ProjectRootDir,
    }))
  return {
    ...project,
    projects,
    options: testDefaults({
      allProjects,
    }),
  }
}

function installProjects (projects: Record<ProjectId, ProjectManifest>): MutatedProject[] {
  return Object.entries(projects)
    .map(([id, manifest]) => ({
      mutation: 'install',
      id,
      manifest,
      rootDir: path.resolve(id) as ProjectRootDir,
    }))
}

test('installing with "catalog:" should work', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
    // Empty second project to create a multi-package workspace.
    {
      name: 'project2',
    },
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: { 'is-positive': '1.0.0' },
    },
  })

  const lockfile = readLockfile()
  expect(lockfile.importers['project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })
})

test('importer to importer dependency with "catalog:" should work', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        project2: 'workspace:*',
      },
    },
    {
      name: 'project2',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: { 'is-positive': '1.0.0' },
    },
  })

  const lockfile = readLockfile()
  expect(lockfile.importers['project2' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })
})

test('importer with different peers uses correct peer', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/has-foo100-peer': 'catalog:',
        // Define a peer with an exact version to ensure the dep above uses
        // this peer.
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    {
      name: 'project2',
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
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        '@pnpm.e2e/has-foo100-peer': '^1.0.0',
      },
    },
  })

  const lockfile = readLockfile()
  expect(lockfile.importers['project1' as ProjectId]?.dependencies?.['@pnpm.e2e/has-foo100-peer']).toEqual({
    specifier: 'catalog:',
    version: `1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/foo', version: '100.0.0' }])}`,
  })
  expect(lockfile.importers['project2' as ProjectId]?.dependencies?.['@pnpm.e2e/has-foo100-peer']).toEqual({
    specifier: 'catalog:',
    //              This version is intentionally different from the one above    ꜜ
    version: `1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/foo', version: '100.1.0' }])}`,
  })
})

test('lockfile contains catalog snapshots', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
    {
      name: 'project2',
      dependencies: {
        'is-negative': 'catalog:',
      },
    },
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        'is-positive': '^1.0.0',
        'is-negative': '^1.0.0',
      },
    },
  })

  const lockfile = readLockfile()
  expect(lockfile.catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },
    },
  })
})

test('lockfile is updated if catalog config changes', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        'is-positive': '=1.0.0',
      },
    },
  })

  expect(readLockfile().importers['project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '1.0.0',
      },
    },
  })

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        'is-positive': '=3.1.0',
      },
    },
  })

  expect(readLockfile().importers['project1' as ProjectId]).toEqual({
    dependencies: {
      'is-positive': {
        specifier: 'catalog:',
        version: '3.1.0',
      },
    },
  })
})

test('lockfile catalog snapshots retain existing entries on --filter', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-negative': 'catalog:',
      },
    },
    {
      name: 'project2',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
  ])

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        'is-positive': '^1.0.0',
        'is-negative': '^1.0.0',
      },
    },
  })

  expect(readLockfile().catalogs).toStrictEqual({
    default: {
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
    },
  })

  // Update catalog definitions so pnpm triggers a rerun.
  await mutateModules(installProjects(projects).slice(1), {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        'is-positive': '=3.1.0',
        'is-negative': '^1.0.0',
      },
    },
  })

  expect(readLockfile().catalogs).toStrictEqual({
    default: {
      // The is-negative snapshot should be carried from the previous install,
      // despite the current filtered install not using it.
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },

      'is-positive': { specifier: '=3.1.0', version: '3.1.0' },
    },
  })
})

test('external dependency using catalog protocol errors', async () => {
  const { options, projects } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/pkg-with-accidentally-published-catalog-protocol': '1.0.0',
      },
    },
  ])

  await expect(() =>
    mutateModules(installProjects(projects), {
      ...options,
      lockfileOnly: true,
    })
  ).rejects.toThrow("@pnpm.e2e/hello-world-js-bin@catalog:foo isn't supported by any available resolver.")
})

test('catalog resolutions should be consistent', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
    {
      name: 'project2',
      dependencies: {},
    },
    {
      name: 'project3',
      dependencies: {},
    },
  ])

  const catalogs = {
    default: {
      'is-positive': '=3.0.0',
    },
  }

  const mutateOpts: MutateModulesOptions = {
    ...options,
    lockfileOnly: true,
    resolutionMode: 'highest',
    catalogs,
  }

  await mutateModules(installProjects(projects), mutateOpts)

  // Change the is-positive catalog entry from =3.0.0 to ^3.0.0 to lock ^3.0.0
  // to the existing 3.0.0 version in the lockfile.
  catalogs.default['is-positive'] = '^3.0.0'
  await mutateModules(installProjects(projects), mutateOpts)
  expect(readLockfile().catalogs).toEqual({
    default: {
      'is-positive': { specifier: '^3.0.0', version: '3.0.0' },
    },
  })

  // Add a different version of is-positive to the lockfile.
  projects['project2' as ProjectId].dependencies = {
    'is-positive': '3.1.0',
  }
  await mutateModules(installProjects(projects), mutateOpts)

  // At this point, both 3.0.0 and 3.1.0 should be in the lockfile, but the
  // catalog entry still resolves to 3.0.0.
  expect(readLockfile()).toEqual(expect.objectContaining({
    catalogs: { default: { 'is-positive': { specifier: '^3.0.0', version: '3.0.0' } } },
    packages: expect.objectContaining({
      'is-positive@3.0.0': expect.objectContaining({}),
      'is-positive@3.1.0': expect.objectContaining({}),
    }),
  }))

  // Adding a new catalog dependency. It should resolve to 3.0.0 instead of 3.1.0, despite resolution-mode=highest.
  projects['project3' as ProjectId].dependencies = {
    'is-positive': 'catalog:',
  }
  await mutateModules(installProjects(projects), mutateOpts)

  // Expect all projects using the catalog specifier (e.g. project1 and project3) to resolve to the same version.
  expect(readLockfile()).toEqual(expect.objectContaining({
    catalogs: { default: { 'is-positive': { specifier: '^3.0.0', version: '3.0.0' } } },
    importers: expect.objectContaining({
      project1: expect.objectContaining({ dependencies: { 'is-positive': { specifier: 'catalog:', version: '3.0.0' } } }),
      project2: expect.objectContaining({ dependencies: { 'is-positive': { specifier: '3.1.0', version: '3.1.0' } } }),
      project3: expect.objectContaining({ dependencies: { 'is-positive': { specifier: 'catalog:', version: '3.0.0' } } }),
    }),
  }))
})

// Similar to the 'catalog resolutions should be consistent' test above, but
// ensures this works for catalog entries using npm aliases.
test('catalog entry using npm alias can be reused', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        '@pnpm.test/is-positive-alias': 'catalog:',
      },
    },
    {
      name: 'project2',
      dependencies: {},
    },
  ])

  const mutateOpts: MutateModulesOptions = {
    ...options,
    lockfileOnly: true,
    catalogs: {
      default: {
        '@pnpm.test/is-positive-alias': 'npm:is-positive@1.0.0',
      },
    },
  }

  await mutateModules(installProjects(projects), mutateOpts)

  // Sanity check that we're recording an expected version specifier.
  expect(readLockfile().catalogs.default?.['@pnpm.test/is-positive-alias']).toEqual({
    specifier: 'npm:is-positive@1.0.0',
    version: '1.0.0',
  })

  // If project2 now reuses a catalog entry with the catalog specifier, the
  // catalog snapshot above should be used and work.
  projects['project2' as ProjectId].dependencies = {
    '@pnpm.test/is-positive-alias': 'catalog:',
  }

  await mutateModules(installProjects(projects), mutateOpts)

  expect(readLockfile()).toEqual(expect.objectContaining({
    catalogs: { default: { '@pnpm.test/is-positive-alias': { specifier: 'npm:is-positive@1.0.0', version: '1.0.0' } } },
    importers: expect.objectContaining({
      project1: expect.objectContaining({ dependencies: { '@pnpm.test/is-positive-alias': { specifier: 'catalog:', version: 'is-positive@1.0.0' } } }),
      project2: expect.objectContaining({ dependencies: { '@pnpm.test/is-positive-alias': { specifier: 'catalog:', version: 'is-positive@1.0.0' } } }),
    }),
  }))
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
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        'is-negative': 'catalog:',
        'is-positive': 'catalog:',
      },
    },
  ])

  const catalogs = {
    default: {
      'is-negative': '=1.0.0',
      'is-positive': '=1.0.0',
    },
  }
  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs,
  })

  {
    const lockfile = readLockfile()
    expect(lockfile.importers['project1' as ProjectId]?.dependencies).toEqual({
      'is-negative': { specifier: 'catalog:', version: '1.0.0' },
      'is-positive': { specifier: 'catalog:', version: '1.0.0' },
    })
    expect(lockfile.catalogs?.default).toStrictEqual({
      'is-negative': { specifier: '=1.0.0', version: '1.0.0' },
      'is-positive': { specifier: '=1.0.0', version: '1.0.0' },
    })
  }

  // Update package.json to no longer depend on is-positive.
  projects['project1' as ProjectId].dependencies = {
    'is-negative': 'catalog:',
  }
  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs,
  })

  {
    const lockfile = readLockfile()
    expect(lockfile.importers['project1' as ProjectId]?.dependencies).toEqual({
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
