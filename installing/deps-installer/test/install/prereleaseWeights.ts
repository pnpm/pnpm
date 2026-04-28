import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { type MutatedProject, mutateModules, type MutateModulesOptions, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

afterEach(async () => {
  await teardownMockAgent()
})

// Regression test for https://github.com/pnpm/pnpm/issues/10626
test('prerelease specifiers do not cause not-yet-used version to be resolved', async () => {
  const rootProject = prepareEmpty()
  const lockfileDir = rootProject.dir()

  const name = '@pnpm.e2e/prerelease'

  const projects: Record<ProjectId, ProjectManifest> = {
    ['a' as ProjectId]: {
      name: 'a',
      dependencies: {
        [name]: '^1.1.0-beta',
      },
    },
    ['b' as ProjectId]: {
      name: 'b',
      dependencies: {
        [name]: '^1.2.0-beta',
      },
    },
    ['c' as ProjectId]: {
      name: 'c',
    },
  }
  const allProjects: ProjectOptions[] = Object.entries(projects)
    .map(([id, manifest]) => ({
      buildIndex: 0,
      manifest,
      rootDir: path.resolve(id) as ProjectRootDir,
    }))
  const options = {
    ...testDefaults(
      { allProjects },
      { retry: { retries: 0 } }
    ),
    lockfileDir,
    lockfileOnly: true,
    resolutionMode: 'highest',
  } satisfies MutateModulesOptions

  const installProjects: MutatedProject[] = Object.entries(projects)
    .map(([id, manifest]) => ({
      mutation: 'install',
      id,
      manifest,
      rootDir: path.resolve(id) as ProjectRootDir,
    }))

  const meta: PackageMeta = {
    name,
    versions: {
      '1.1.0-beta': {
        name,
        version: '1.1.0-beta',
        // Generated locally through: echo '1.1.0-beta' | sha1sum
        dist: { shasum: '7957736c00bc1e5a875e5e4f8f48d8f5a3830866', tarball: `${options.registries.default}/${name}-1.1.0-beta.tgz` },
      },
      '1.2.0-beta': {
        name,
        version: '1.2.0-beta',
        dist: { shasum: '50c0586b05b59205f39610d63cc38ea04954182c', tarball: `${options.registries.default}/${name}-1.2.0-beta.tgz` },
      },
    },
    'dist-tags': {
      latest: '1.2.0-beta',
    },
  }

  await setupMockAgent()
  const registryUrl = options.registries.default.replace(/\/$/, '')
  // cspell:disable-next-line
  const metadataPath = '/@pnpm.e2e%2Fprerelease'

  getMockAgent().get(registryUrl)
    .intercept({ path: metadataPath, method: 'GET' })
    .reply(200, meta)

  await mutateModules(installProjects, options)

  {
    const lockfile = rootProject.readLockfile()
    expect(lockfile.importers['a' as ProjectId].dependencies?.[name]).toEqual({
      specifier: '^1.1.0-beta',
      version: '1.1.0-beta',
    })
    expect(lockfile.importers['b' as ProjectId].dependencies?.[name]).toEqual({
      specifier: '^1.2.0-beta',
      version: '1.2.0-beta',
    })
  }

  // Simulate publishing a new 1.2.0 version.
  meta.versions['1.2.0'] = {
    name,
    version: '1.2.0',
    dist: { shasum: 'f95c23882c82328c872ac94af630c49ae57f37bb', tarball: `${options.registries.default}/${name}-1.2.0.tgz` },
  }
  meta['dist-tags'].latest = '1.2.0'

  options.storeController.clearResolutionCache()

  getMockAgent().get(registryUrl)
    .intercept({ path: metadataPath, method: 'GET' })
    .reply(200, meta)

  projects['c' as ProjectId].dependencies = { [name]: '^1.2.0-beta' }
  await mutateModules(installProjects, options)

  const lockfile = rootProject.readLockfile()
  expect(lockfile.importers['c' as ProjectId].dependencies?.[name]).toEqual({
    specifier: '^1.2.0-beta',
    version: '1.2.0-beta',
  })
})
