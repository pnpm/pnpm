import { createPeerDepGraphHash } from '@pnpm/dependency-path'
import { type ProjectRootDir, type ProjectId, type ProjectManifest } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type MutatedProject, type ProjectOptions, type MutateModulesOptions } from '@pnpm/core'
import { type CatalogSnapshots } from '@pnpm/lockfile.types'
import { jest } from '@jest/globals'
import { loadJsonFileSync } from 'load-json-file'
import path from 'path'
import { testDefaults } from './utils/index.js'

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  originalModule.logger.warn = jest.fn()
  return originalModule
})

const { logger } = await import('@pnpm/logger')
const { mutateModules, addDependenciesToPackage } = await import('@pnpm/core')

function preparePackagesAndReturnObjects (manifests: Array<ProjectManifest & Required<Pick<ProjectManifest, 'name'>>>) {
  const project = prepareEmpty()
  const lockfileDir = process.cwd()
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
    options: {
      ...testDefaults({
        allProjects,
      }),
      lockfileDir,
    },
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
    version: `1.0.0${createPeerDepGraphHash([{ name: '@pnpm.e2e/foo', version: '100.0.0' }])}`,
  })
  expect(lockfile.importers['project2' as ProjectId]?.dependencies?.['@pnpm.e2e/has-foo100-peer']).toEqual({
    specifier: 'catalog:',
    //              This version is intentionally different from the one above    ꜜ
    version: `1.0.0${createPeerDepGraphHash([{ name: '@pnpm.e2e/foo', version: '100.1.0' }])}`,
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

test('frozen lockfile error is thrown if catalog config changes', async () => {
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

  const frozenLockfileMutation = mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    frozenLockfile: true,
    catalogs: {
      default: {
        'is-positive': '=3.1.0',
      },
    },
  })

  await expect(frozenLockfileMutation).rejects.toThrow('Cannot proceed with the frozen installation. The current "catalogs" configuration doesn\'t match the value found in the lockfile')
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

// Regression test for https://github.com/pnpm/pnpm/issues/8638
test('lockfile catalog snapshots do not contain stale references on --filter', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {},
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
    catalogs: {
      default: {
        'is-positive': '^1.0.0',
      },
    },
  })

  expect(readLockfile().catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
    },
  })

  // This test updates the catalog entry in project2, but only performs a
  // filtered install on project1. The lockfile catalog snapshots for project2
  // should still be updated despite it not being part of the filtered install.
  const onlyProject1 = installProjects(projects).slice(0, 1)
  expect(onlyProject1).toMatchObject([{ id: 'project1' }])

  await mutateModules(onlyProject1, {
    ...options,
    catalogs: {
      default: {
        'is-positive': '=3.1.0',
      },
    },
  })

  expect(readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        'is-positive': { specifier: '=3.1.0', version: '3.1.0' },
      },
    },
    importers: expect.objectContaining({
      project1: {},
      project2: expect.objectContaining({
        dependencies: {
          // project 2 should be updated even though it wasn't part of the
          // filtered install. This is due to a filtered install updating
          // the lockfile first: https://github.com/pnpm/pnpm/pull/8183
          'is-positive': { specifier: 'catalog:', version: '3.1.0' },
        },
      }),
    }),
  }))

  // is-positive was not updated because only dependencies of project1 were.
  const pathToIsPositivePkgJson = path.join(options.allProjects[1].rootDir!, 'node_modules/is-positive/package.json')
  expect(loadJsonFileSync<ProjectManifest>(pathToIsPositivePkgJson)?.version).toBe('1.0.0')

  await mutateModules(installProjects(projects), {
    ...options,
    catalogs: {
      default: {
        'is-positive': '=3.1.0',
      },
    },
  })

  // is-positive is now updated because a full install took place.
  expect(loadJsonFileSync<ProjectManifest>(pathToIsPositivePkgJson)?.version).toBe('3.1.0')
})

// Regression test for https://github.com/pnpm/pnpm/issues/9112
test('dedupe-peer-dependents=false with --filter does not erase catalog snapshots', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {},
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
    dedupePeerDependents: false,
    catalogs: {
      default: {
        'is-positive': '1.0.0',
      },
    },
  })

  expect(readLockfile().catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '1.0.0', version: '1.0.0' },
    },
  })

  // Perform a filtered install with only project 1. The catalog protocol usage
  // in project 2 should be retained.
  const onlyProject1 = installProjects(projects).slice(0, 1)
  expect(onlyProject1).toMatchObject([{ id: 'project1' }])
  await mutateModules(onlyProject1, {
    ...options,
    lockfileOnly: true,
    dedupePeerDependents: false,
    catalogs: {
      default: {
        // Modify the original specifier above from "1.0.0" to "^1.0.0" in order
        // to force a resolution instead of a frozen install.
        'is-positive': '^1.0.0',
      },
    },
  })

  // The catalogs snapshot section was erased in the bug report from
  // https://github.com/pnpm/pnpm/issues/9112 when dedupe-peer-dependents=false.
  expect(readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
      },
    },
    importers: expect.objectContaining({
      project1: {},
      project2: expect.objectContaining({
        dependencies: {
          'is-positive': { specifier: 'catalog:', version: '1.0.0' },
        },
      }),
    }),
  }))
})

// Regression test for https://github.com/pnpm/pnpm/issues/8639
test('--fix-lockfile with --filter does not erase catalog snapshots', async () => {
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

  const catalogs = {
    default: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
  }

  const expectedCatalogsSnapshot: CatalogSnapshots = {
    default: {
      'is-negative': { specifier: '^1.0.0', version: '1.0.0' },
      'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
    },
  }

  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs,
  })

  // Sanity check this test is set up correctly.
  expect(readLockfile().catalogs).toStrictEqual(expectedCatalogsSnapshot)

  // The catalogs snapshot should still be the same after performing a filtered
  // install with --fix-lockfile.
  const onlyProject1 = installProjects(projects).slice(0, 1)
  expect(onlyProject1).toMatchObject([{ id: 'project1' }])

  await mutateModules(onlyProject1, {
    ...options,
    lockfileOnly: true,
    fixLockfile: true,
    catalogs,
  })

  expect(readLockfile().catalogs).toStrictEqual(expectedCatalogsSnapshot)
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
  ).rejects.toThrow("\"@pnpm.e2e/hello-world-js-bin@catalog:foo\" isn't supported by any available resolver.")
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
  expect(readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: { default: { 'is-positive': { specifier: '^3.0.0', version: '3.0.0' } } },
    packages: expect.objectContaining({
      'is-positive@3.0.0': expect.any(Object),
      'is-positive@3.1.0': expect.any(Object),
    }),
  }))

  // Adding a new catalog dependency. It should resolve to 3.0.0 instead of 3.1.0, despite resolution-mode=highest.
  projects['project3' as ProjectId].dependencies = {
    'is-positive': 'catalog:',
  }
  await mutateModules(installProjects(projects), mutateOpts)

  // Expect all projects using the catalog specifier (e.g. project1 and project3) to resolve to the same version.
  expect(readLockfile()).toMatchObject({
    catalogs: { default: { 'is-positive': { specifier: '^3.0.0', version: '3.0.0' } } },
    importers: {
      project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '3.0.0' } } },
      project2: { dependencies: { 'is-positive': { specifier: '3.1.0', version: '3.1.0' } } },
      project3: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '3.0.0' } } },
    },
  })
})

// Similar to the test above, but ensure the behavior holds for cataloged
// dependencies that have peer dependencies. If the peer suffix ends up being
// different for a new usage of the catalog protocol, the version of the
// dependency should still be the same.
test('catalog resolutions should be consistent with peer dependencies', async () => {
  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/abc': 'catalog:',

        // The @pnpm.e2e/abc package has peer dependencies on all of these.
        // Adding them for explicitness.
        '@pnpm.e2e/peer-a': '1.0.0',
        '@pnpm.e2e/peer-b': '1.0.0',
        '@pnpm.e2e/peer-c': '1.0.0',
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
      '@pnpm.e2e/abc': '1.0.0',
    },
  }

  const mutateOpts: MutateModulesOptions = {
    ...options,
    lockfileOnly: true,
    resolutionMode: 'highest',
    catalogs,
  }

  await mutateModules(installProjects(projects), mutateOpts)

  // Updating the specifier to * so it could feasibly match @pnpm.e2e/abc@2.0.0.
  // This test will ensure it stays on @pnpm.e2e/abc@1.0.0 for the catalog.
  //
  // At the time of writing (May 2025), the registry mock has @pnpm.e2e/abc
  // version 1.0.0 and 2.0.0.
  catalogs.default['@pnpm.e2e/abc'] = '*'
  await mutateModules(installProjects(projects), mutateOpts)
  expect(readLockfile().catalogs).toEqual({
    default: {
      '@pnpm.e2e/abc': { specifier: '*', version: '1.0.0' },
    },
  })

  // Add a different version of @pnpm.e2e/abc to the lockfile.
  projects['project2' as ProjectId].dependencies = {
    '@pnpm.e2e/abc': '2.0.0',
    '@pnpm.e2e/peer-a': '1.0.0',
    '@pnpm.e2e/peer-b': '1.0.0',
    '@pnpm.e2e/peer-c': '1.0.0',
  }
  await mutateModules(installProjects(projects), mutateOpts)

  expect(readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: { default: { '@pnpm.e2e/abc': { specifier: '*', version: '1.0.0' } } },
    packages: expect.objectContaining({
      // At this point, both 1.0.0 and 2.0.0 should be in the lockfile, but the
      // catalog entry still resolves to 1.0.0.
      '@pnpm.e2e/abc@1.0.0': expect.any(Object),
      '@pnpm.e2e/abc@2.0.0': expect.any(Object),

      // This is a regular dependency of @pnpm.e2e/abc.
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': expect.any(Object),

      '@pnpm.e2e/peer-a@1.0.0': expect.any(Object),
      '@pnpm.e2e/peer-b@1.0.0': expect.any(Object),
      '@pnpm.e2e/peer-c@1.0.0': expect.any(Object),
    }),
  }))

  // Adding a new catalog dependency. It should resolve to 1.0.0 instead of 2.0.0, despite resolution-mode=highest.
  projects['project3' as ProjectId].dependencies = {
    '@pnpm.e2e/abc': 'catalog:',
    // Compared to project1, this is intentionally changed from "1.0.0" to
    // "1.0.1" so @pnpm.e2e/abc resolves with a different peer suffix.
    '@pnpm.e2e/peer-a': '1.0.1',
    '@pnpm.e2e/peer-b': '1.0.0',
    '@pnpm.e2e/peer-c': '1.0.0',
  }
  await mutateModules(installProjects(projects), mutateOpts)

  // Expect all projects using the catalog specifier (e.g. project1 and
  // project3) to resolve to the same version. They will have different peers
  // suffixes since @pnpm.e2e/peer-a will be on different versions, but the
  // version of @pnpm.e2e/abc should be 1.0.0.
  expect(readLockfile()).toMatchObject({
    catalogs: { default: { '@pnpm.e2e/abc': { specifier: '*', version: '1.0.0' } } },
    importers: {
      project1: {
        dependencies: {
          '@pnpm.e2e/abc': { specifier: 'catalog:', version: '1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)' },
          '@pnpm.e2e/peer-a': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/peer-b': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/peer-c': { specifier: '1.0.0', version: '1.0.0' },
        },
      },
      project2: {
        dependencies: {
          '@pnpm.e2e/abc': { specifier: '2.0.0', version: '2.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)' },
          '@pnpm.e2e/peer-a': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/peer-b': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/peer-c': { specifier: '1.0.0', version: '1.0.0' },
        },
      },
      project3: {
        dependencies: {
          '@pnpm.e2e/abc': { specifier: 'catalog:', version: '1.0.0(@pnpm.e2e/peer-a@1.0.1)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)' },
          '@pnpm.e2e/peer-a': { specifier: '1.0.1', version: '1.0.1' },
          '@pnpm.e2e/peer-b': { specifier: '1.0.0', version: '1.0.0' },
          '@pnpm.e2e/peer-c': { specifier: '1.0.0', version: '1.0.0' },
        },
      },
    },
  })
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

  expect(readLockfile()).toMatchObject({
    catalogs: { default: { '@pnpm.test/is-positive-alias': { specifier: 'npm:is-positive@1.0.0', version: '1.0.0' } } },
    importers: {
      project1: { dependencies: { '@pnpm.test/is-positive-alias': { specifier: 'catalog:', version: 'is-positive@1.0.0' } } },
      project2: { dependencies: { '@pnpm.test/is-positive-alias': { specifier: 'catalog:', version: 'is-positive@1.0.0' } } },
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

// Regression test for https://github.com/pnpm/pnpm/issues/8715
//
// Catalogs on injected deps require more consideration since the injected dep
// is no longer seen as an "importer". The catalog protocol is traditionally
// only for "importers" (i.e. packages matching the `packages` filter in
// pnpm-workspace.yaml).
//
// Since injected deps copy the workspace package into the node_modules/.pnpm
// dir, a bit more work has to be done to make catalogs usable on these unique
// packages.
//
// Example of a package at packages/project2 getting "injected".
//
//   node_modules/.pnpm/project2@file+packages+project2/node_modules/project2
//
test('catalogs work in injected dep', async () => {
  expect.hasAssertions()

  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        project2: 'workspace:*',
      },
      dependenciesMeta: {
        project2: { injected: true },
      },
    },
    {
      name: 'project2',
      dependencies: {
        'is-positive': 'catalog:',
      },
    },
  ])

  const install = () => mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    // This setting turns injected deps into regular symlinked workspace
    // packages if peer dependencies aren't resolved differently.
    dedupeInjectedDeps: false,
    catalogs: {
      default: { 'is-positive': '1.0.0' },
    },
  })

  // This should run without "is-positive@catalog: isn't supported by any
  // available resolver." errors.
  await expect(install()).resolves.not.toThrow()

  const lockfile = readLockfile()

  // The resolved catalogs should be correct.
  expect(lockfile.catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '1.0.0', version: '1.0.0' },
    },
  })

  expect(lockfile.importers).toEqual({
    // Check that project2 was indeed injected into project1. Otherwise this
    // test wouldn't be checking the correct scenario.
    project1: {
      dependencies: {
        project2: { specifier: 'workspace:*', version: 'file:project2' },
      },
      dependenciesMeta: {
        project2: { injected: true },
      },
    },
    project2: {
      dependencies: {
        'is-positive': { specifier: 'catalog:', version: '1.0.0' },
      },
    },
  })

  // Double check the correct version of is-positive as requested from the
  // catalog was installed and not the latest.
  expect(lockfile.snapshots).toStrictEqual({
    'is-positive@1.0.0': {},
    'project2@file:project2': {
      dependencies: { 'is-positive': '1.0.0' },
    },
  })
})

test('catalogs work when inject-workspace-packages=true', async () => {
  expect.hasAssertions()

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

  const install = () => mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    // This setting turns injected deps into regular symlinked workspace
    // packages if peer dependencies aren't resolved differently.
    dedupeInjectedDeps: false,
    injectWorkspacePackages: true,
    catalogs: {
      default: { 'is-positive': '1.0.0' },
    },
  })

  // This should run without "is-positive@catalog: isn't supported by any
  // available resolver." errors.
  await expect(install()).resolves.not.toThrow()

  const lockfile = readLockfile()

  // The resolved catalogs should be correct.
  expect(lockfile.catalogs).toStrictEqual({
    default: {
      'is-positive': { specifier: '1.0.0', version: '1.0.0' },
    },
  })

  expect(lockfile.importers).toEqual({
    // Check that project2 was indeed injected into project1. Otherwise this
    // test wouldn't be checking the correct scenario.
    project1: {
      dependencies: {
        project2: { specifier: 'workspace:*', version: 'file:project2' },
      },
    },
    project2: {
      dependencies: {
        'is-positive': { specifier: 'catalog:', version: '1.0.0' },
      },
    },
  })

  // Double check the correct version of is-positive as requested from the
  // catalog was installed and not the latest.
  expect(lockfile.snapshots).toStrictEqual({
    'is-positive@1.0.0': {},
    'project2@file:project2': {
      dependencies: { 'is-positive': '1.0.0' },
    },
  })
})

describe('dedupe', () => {
  test('catalogs are deduped when running pnpm dedupe', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
      {
        name: 'project1',
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:',
        },
      },
      {
        name: 'project2',
      },
    ])

    const catalogs = {
      default: { '@pnpm.e2e/foo': '100.0.0' },
    }

    await mutateModules(installProjects(projects), {
      ...options,
      lockfileOnly: true,
      catalogs,
    })

    // Add a ^ to the existing 100.0.0 specifier. Despite higher versions
    // published to the registry mock, pnpm should prefer the existing 100.0.0
    // specifier in the lockfile.
    catalogs.default['@pnpm.e2e/foo'] = '^100.0.0'

    await mutateModules(installProjects(projects), {
      ...options,
      lockfileOnly: true,
      catalogs,
    })

    // Check that our testing state is set up correctly and that the addition of
    // ^ above didn't accidentally upgrade.
    expect(Object.keys(readLockfile().packages)).toEqual(['@pnpm.e2e/foo@100.0.0'])

    projects['project2' as ProjectId].dependencies = {
      '@pnpm.e2e/foo': '100.1.0',
    }

    await mutateModules(installProjects(projects), {
      ...options,
      lockfileOnly: true,
      catalogs,
    })

    // Due to project2 directly adding a new dependency on @pnpm.e2e/foo version
    // 100.1.0, both versions should now exist in the lockfile.
    const lockfile = readLockfile()
    expect(Object.keys(lockfile.packages)).toEqual(['@pnpm.e2e/foo@100.0.0', '@pnpm.e2e/foo@100.1.0'])
    expect(lockfile.catalogs.default['@pnpm.e2e/foo'].version).toBe('100.0.0')

    // Perform a dedupe and expect the catalog version to update.
    await mutateModules(installProjects(projects), {
      ...options,
      dedupe: true,
      lockfileOnly: true,
      catalogs,
    })
    const dedupedLockfile = readLockfile()
    expect(Object.keys(dedupedLockfile.packages)).toEqual(['@pnpm.e2e/foo@100.1.0'])
    expect(dedupedLockfile.catalogs.default['@pnpm.e2e/foo'].version).toBe('100.1.0')
  })
})

describe('add', () => {
  test('adding is-positive@catalog: works', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@catalog:'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: { 'is-positive': '1.0.0' },
        },
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { 'is-positive': { specifier: '1.0.0', version: '1.0.0' } } },
      importers: { project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } } },
      packages: { 'is-positive@1.0.0': expect.any(Object) },
    })
  })

  test('adding no specific version will use catalog if present', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: { 'is-positive': '1.0.0' },
        },
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { 'is-positive': { specifier: '1.0.0', version: '1.0.0' } } },
      importers: { project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } } },
      packages: { 'is-positive@1.0.0': expect.any(Object) },
    })
  })

  // Regression test for https://github.com/pnpm/pnpm/issues/9759
  test('adding new usage of default catalog does not mutate catalog entries', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
      {
        name: 'project1',
        dependencies: {
          '@pnpm.e2e/foo': 'catalog:',
        },
      },
      {
        name: 'project2',
      },
    ])

    const catalogs = {
      default: { '@pnpm.e2e/foo': '^100.0.0' },
    }

    await mutateModules(installProjects(projects), {
      ...options,
      lockfileOnly: true,
      catalogs,
    })

    await addDependenciesToPackage(
      projects['project2' as ProjectId],
      ['@pnpm.e2e/foo'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project2'),
        lockfileOnly: true,
        allowNew: true,
        catalogs,
      })

    const lockfile = readLockfile()

    // This is the specific condition we're regression testing for. The
    // specifier used in the original catalog entry should not be modified.
    expect(lockfile.catalogs.default['@pnpm.e2e/foo'].specifier).toEqual(catalogs.default['@pnpm.e2e/foo'])

    // Sanity check that the rest of the lockfile has expected contents.
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { '@pnpm.e2e/foo': { specifier: '^100.0.0', version: '100.0.0' } } },
      importers: {
        project1: { dependencies: { '@pnpm.e2e/foo': { specifier: 'catalog:', version: '100.0.0' } } },
        project2: { dependencies: { '@pnpm.e2e/foo': { specifier: 'catalog:', version: '100.0.0' } } },
      },
      packages: { '@pnpm.e2e/foo@100.0.0': expect.any(Object) },
    })
  })

  test('adding specific version equal to catalog version will use catalog if present', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@1.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: { 'is-positive': '1.0.0' },
        },
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { 'is-positive': { specifier: '1.0.0', version: '1.0.0' } } },
      importers: { project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } } },
      packages: { 'is-positive@1.0.0': expect.any(Object) },
    })
  })

  test('adding different version than the catalog will not use catalog', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@2.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: { 'is-positive': '1.0.0' },
        },
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': '2.0.0',
      },
    })
    expect(readLockfile().packages).toStrictEqual({
      'is-positive@2.0.0': expect.any(Object),
    })
  })

  test('adding with catalogMode: strict will add to or use from catalog', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@1.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: {},
        },
        catalogMode: 'strict',
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { 'is-positive': { specifier: '1.0.0', version: '1.0.0' } } },
      importers: { project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } } },
      packages: { 'is-positive@1.0.0': expect.any(Object) },
    })
  })

  // Regression test for https://github.com/pnpm/pnpm/issues/10176
  // When re-adding a dependency that already exists in the catalog with catalogMode: strict,
  // the catalog entry should preserve the original version specifier, not become 'catalog:'
  test('re-adding existing catalog dependency with catalogMode: strict preserves catalog specifier', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    }])

    // First, install the existing dependency with the catalog
    const mutateOpts = {
      ...options,
      lockfileOnly: true,
      catalogs: {
        default: { 'is-positive': '^1.0.0' },
      },
      catalogMode: 'strict' as const,
    }

    await mutateModules(installProjects(projects), mutateOpts)

    // Verify initial state
    expect(readLockfile().catalogs?.default?.['is-positive']).toEqual({
      specifier: '^1.0.0',
      version: '1.0.0',
    })

    // Now re-add the same dependency (simulating 'pnpm add is-positive' from a subpackage)
    const { updatedManifest, updatedCatalogs } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive'],
      {
        ...mutateOpts,
        dir: path.join(options.lockfileDir, 'project1'),
        allowNew: true,
      })

    // The manifest should still use catalog:
    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })

    // The catalog should preserve the original specifier, NOT become 'catalog:'
    // This is the bug fix - previously it would incorrectly write 'catalog:' to the catalog
    if (updatedCatalogs?.default?.['is-positive']) {
      expect(updatedCatalogs.default['is-positive']).not.toBe('catalog:')
      expect(updatedCatalogs.default['is-positive']).toMatch(/^\^?\d/)
    }

    // The lockfile should have the correct catalog specifier
    const lockfile = readLockfile()
    expect(lockfile.catalogs?.default?.['is-positive']?.specifier).not.toBe('catalog:')
  })

  test('adding with catalogMode: prefer will add to or use from catalog', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {},
    }])

    const { updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@1.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: {},
        },
        catalogMode: 'prefer',
      })

    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    })
    expect(readLockfile()).toMatchObject({
      catalogs: { default: { 'is-positive': { specifier: '1.0.0', version: '1.0.0' } } },
      importers: { project1: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } } },
      packages: { 'is-positive@1.0.0': expect.any(Object) },
    })
  })

  test('adding mismatched version with catalogMode: strict will error', async () => {
    const { options, projects } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    }])

    await expect(addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@2.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        lockfileOnly: true,
        allowNew: true,
        catalogs: {
          default: {
            'is-positive': '1.0.0',
          },
        },
        catalogMode: 'strict',
      })
    ).rejects.toThrow()
  })

  test('adding mismatched version with catalogMode: prefer will warn and use direct', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        'is-positive': 'catalog:',
      },
    }, {
      name: 'project2',
      dependencies: {
        'is-positive': 'catalog:',
      },
    }])

    options.catalogs = {
      default: {
        'is-positive': '1.0.0',
      },
    }
    options.lockfileOnly = true

    await mutateModules(installProjects(projects), options)

    expect(options.catalogs).toStrictEqual({
      default: {
        'is-positive': '1.0.0',
      },
    })

    let installResult = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['is-positive@2.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project1'),
        allowNew: true,
        catalogMode: 'prefer',
      })

    expect(installResult.updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        'is-positive': '2.0.0',
      },
    })

    expect(logger.warn).toHaveBeenCalled()
    expect(readLockfile().importers).toStrictEqual(
      {
        project1: { dependencies: { 'is-positive': { specifier: '2.0.0', version: '2.0.0' } } },
        project2: { dependencies: { 'is-positive': { specifier: 'catalog:', version: '1.0.0' } } },
      }
    )
    expect(readLockfile().packages).toMatchObject(
      { 'is-positive@2.0.0': expect.any(Object), 'is-positive@1.0.0': expect.any(Object) }
    )
    expect(options.catalogs).toStrictEqual(
      { default: { 'is-positive': '1.0.0' } }
    )

    installResult = await addDependenciesToPackage(
      projects['project2' as ProjectId],
      ['is-positive@2.0.0'],
      {
        ...options,
        dir: path.join(options.lockfileDir, 'project2'),
        allowNew: true,
        catalogMode: 'prefer',
      })

    expect(installResult.updatedManifest).toEqual({
      name: 'project2',
      dependencies: {
        'is-positive': '2.0.0',
      },
    })

    expect(logger.warn).toHaveBeenCalled()
    expect(readLockfile().importers).toStrictEqual(
      {
        project1: { dependencies: { 'is-positive': { specifier: '2.0.0', version: '2.0.0' } } },
        project2: { dependencies: { 'is-positive': { specifier: '2.0.0', version: '2.0.0' } } },
      }
    )
    expect(readLockfile().packages).toMatchObject(
      { 'is-positive@2.0.0': expect.any(Object) }
    )
    expect(options.catalogs).toStrictEqual(
      { default: { 'is-positive': '1.0.0' } }
    )
  })
})

describe('update', () => {
  // Many of the update tests use @pnpm.e2e/foo, which has the following
  // versions currently published to the https://github.com/pnpm/registry-mock
  //
  //   - 1.0.0
  //   - 1.1.0
  //   - 1.2.0
  //   - 1.3.0
  //   - 2.0.0
  //   - 100.0.0
  //   - 100.1.0
  //
  // The @pnpm.e2e/foo package is used rather than public packages like
  // is-positive since public packages can release new versions and break the
  // tests here.

  test('update works on cataloged dependency', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    }])

    const mutateOpts = {
      ...options,
      lockfileOnly: true,
      // Start by using 1.0.0 as the specifier. We'll then change this to ^1.0.0
      // and to test pnpm properly updates from 1.0.0 to 1.3.0.
      catalogs: {
        default: { '@pnpm.e2e/foo': '1.0.0' },
      },
    }

    await mutateModules(installProjects(projects), mutateOpts)

    // Changing the catalog from 1.0.0 to ^1.0.0. This should still lock to the
    // existing 1.0.0 version despite version 1.3.0 available on the registry.
    mutateOpts.catalogs.default['@pnpm.e2e/foo'] = '^1.0.0'
    await mutateModules(installProjects(projects), mutateOpts)

    // Sanity check that the @pnpm.e2e/foo dependency is installed on the older
    // requested version.
    expect(readLockfile().catalogs.default).toEqual({
      '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.0.0' },
    })

    const { updatedCatalogs, updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['@pnpm.e2e/foo'],
      {
        ...mutateOpts,
        dir: path.join(options.lockfileDir, 'project1'),
        update: true,
      })

    // Expecting the manifest to remain unchanged after running an update. The
    // change should be reflected in the returned updatedCatalogs object
    // instead.
    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    })
    expect(updatedCatalogs).toEqual({
      default: {
        '@pnpm.e2e/foo': '^1.3.0',
      },
    })

    // The lockfile should also contain the updated ^1.3.0 reference.
    const lockfile = readLockfile()
    expect(lockfile.catalogs).toEqual({
      default: { '@pnpm.e2e/foo': { specifier: '^1.3.0', version: '1.3.0' } },
    })

    // Ensure the old 1.0.0 version is no longer used.
    expect(Object.keys(lockfile.snapshots)).toEqual(['@pnpm.e2e/foo@1.3.0'])
  })

  test('update works on named catalog', async () => {
    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:foo',
      },
    }])

    // Start by using 1.0.0 as the specifier. We'll then change this to ^1.0.0
    // and to test pnpm properly updates from 1.0.0 to 1.3.0.
    const mutateOpts = {
      ...options,
      lockfileOnly: true,
      catalogs: {
        foo: { '@pnpm.e2e/foo': '1.0.0' },
      },
    }

    await mutateModules(installProjects(projects), mutateOpts)

    // Changing the catalog from 1.0.0 to ^1.0.0. This should still lock to the
    // existing 1.0.0 version despite version 1.3.0 available on the registry.
    mutateOpts.catalogs.foo['@pnpm.e2e/foo'] = '^1.0.0'
    await mutateModules(installProjects(projects), mutateOpts)

    // Sanity check that the @pnpm.e2e/foo dependency is installed on the older
    // requested version.
    expect(readLockfile().catalogs.foo).toEqual({
      '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.0.0' },
    })

    const { updatedCatalogs, updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['@pnpm.e2e/foo'],
      {
        ...mutateOpts,
        dir: path.join(options.lockfileDir, 'project1'),
        update: true,
      })

    // Expecting the manifest to remain unchanged after running an update. The
    // change should be reflected in the returned updatedCatalogs object
    // instead.
    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:foo',
      },
    })
    expect(updatedCatalogs).toEqual({
      foo: {
        '@pnpm.e2e/foo': '^1.3.0',
      },
    })

    // The lockfile should also contain the updated ^1.3.0 reference.
    const lockfile = readLockfile()
    expect(lockfile.catalogs).toEqual({
      foo: { '@pnpm.e2e/foo': { specifier: '^1.3.0', version: '1.3.0' } },
    })

    // Ensure the old 1.0.0 version is no longer used.
    expect(Object.keys(lockfile.snapshots)).toEqual(['@pnpm.e2e/foo@1.3.0'])
  })

  test('update --latest works on cataloged dependency', async () => {
    await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    }])

    const catalogs = {
      default: { '@pnpm.e2e/foo': '1.0.0' },
    }

    const mutateOpts = {
      ...options,
      lockfileOnly: true,
      catalogs,
    }

    await mutateModules(installProjects(projects), mutateOpts)

    // Sanity check that the @pnpm.e2e/foo dependency is installed on the older
    // requested version.
    expect(readLockfile().catalogs.default).toEqual({
      '@pnpm.e2e/foo': { specifier: '1.0.0', version: '1.0.0' },
    })

    const { updatedCatalogs, updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['@pnpm.e2e/foo'],
      {
        ...mutateOpts,
        dir: path.join(process.cwd(), 'project1'),
        allowNew: false,
        update: true,
        updateToLatest: true,
      })

    // Expecting the manifest to remain unchanged after running an update. The
    // change should be reflected in the returned updatedCatalogs object
    // instead.
    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    })
    expect(updatedCatalogs).toEqual({
      default: {
        '@pnpm.e2e/foo': '100.1.0',
      },
    })

    expect(Object.keys(readLockfile().snapshots)).toEqual(['@pnpm.e2e/foo@100.1.0'])
  })

  // This test will update @pnpm.e2e/bar, but make sure @pnpm.e2e/foo is
  // untouched. On the registry-mock, the versions for @pnpm.e2e/bar are:
  //
  //   - 100.0.0
  //   - 100.1.0
  test('update only affects matching filter', async () => {
    await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })
    await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

    const { options, projects, readLockfile } = preparePackagesAndReturnObjects([{
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
        '@pnpm.e2e/bar': 'catalog:',
      },
    }])

    const mutateOpts = {
      ...options,
      lockfileOnly: true,
      catalogs: {
        default: {
          // Start by using exact versions for specifiers. We'll then change this to be a range below.
          '@pnpm.e2e/foo': '1.0.0',
          '@pnpm.e2e/bar': '100.0.0',
        },
      },
    }

    await mutateModules(installProjects(projects), mutateOpts)

    // Adding ^ to the catalog config entries. This allows the update process to
    // consider newer versions to update to for this test.
    mutateOpts.catalogs.default['@pnpm.e2e/foo'] = '^1.0.0'
    mutateOpts.catalogs.default['@pnpm.e2e/bar'] = '^100.0.0'
    await mutateModules(installProjects(projects), mutateOpts)

    // Sanity check dependencies are still installed on older requested version
    // and not accidentally updated due to adding ^ above.
    expect(readLockfile().catalogs.default).toEqual({
      '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.0.0' },
      '@pnpm.e2e/bar': { specifier: '^100.0.0', version: '100.0.0' },
    })

    const { updatedCatalogs, updatedManifest } = await addDependenciesToPackage(
      projects['project1' as ProjectId],
      ['@pnpm.e2e/bar'],
      {
        ...mutateOpts,
        dir: path.join(options.lockfileDir, 'project1'),
        update: true,
        updateMatching: (pkgName) => pkgName === '@pnpm.e2e/bar',
      })

    // Expecting the manifest to remain unchanged after running an update. The
    // change should be reflected in the returned updatedCatalogs object
    // instead.
    expect(updatedManifest).toEqual({
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
        '@pnpm.e2e/bar': 'catalog:',
      },
    })
    expect(updatedCatalogs).toEqual({
      default: {
        '@pnpm.e2e/bar': '^100.1.0',
      },
    })

    // The lockfile should also contain the updated ^100.1.0 reference.
    const lockfile = readLockfile()
    expect(lockfile).toEqual(expect.objectContaining({
      catalogs: {
        default: {
          '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.0.0' },
          '@pnpm.e2e/bar': { specifier: '^100.1.0', version: '100.1.0' },
        },
      },
      packages: {
        '@pnpm.e2e/foo@1.0.0': expect.objectContaining({}),
        '@pnpm.e2e/bar@100.1.0': expect.objectContaining({}),
      },
    }))

    // Ensure the old 1.0.0 version is no longer used.
    expect(Object.keys(lockfile.snapshots)).toEqual([
      '@pnpm.e2e/bar@100.1.0',
      '@pnpm.e2e/foo@1.0.0',
    ])
  })
})

test('catalogs work in overrides', async () => {
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  const overrides: Record<string, string> = {
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'catalog:',
    '@pnpm.e2e/bar@^100.0.0': 'catalog:',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': 'catalog:',
  }

  const { options, projects, readLockfile } = preparePackagesAndReturnObjects([
    {
      name: 'project1',
      dependencies: {
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/foobar': '100.0.0',
        '@pnpm.e2e/foobarqar': '1.0.0',
      },
    },
    // Empty second project to create a multi-package workspace.
    {
      name: 'project2',
    },
  ])

  const catalogs = {
    default: {
      '@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar': '100.1.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    },
  }
  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs,
    overrides,
  })

  let lockfile = readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('@pnpm.e2e/qar@100.0.0')
  expect(lockfile.snapshots['@pnpm.e2e/foobar@100.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('100.0.0')
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/bar@100.1.0'])
  expect(lockfile.overrides).toStrictEqual({
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
    '@pnpm.e2e/bar@^100.0.0': '100.1.0',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  })

  catalogs.default['@pnpm.e2e/bar'] = '100.0.0'
  await mutateModules(installProjects(projects), {
    ...options,
    lockfileOnly: true,
    catalogs,
    overrides,
  })

  lockfile = readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/bar@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/bar@100.1.0'])
  expect(lockfile.overrides).toStrictEqual({
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
    '@pnpm.e2e/bar@^100.0.0': '100.0.0',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  })
})
