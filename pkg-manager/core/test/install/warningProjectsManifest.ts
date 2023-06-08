import path from 'path'
import { logger } from '@pnpm/logger'
import { testDefaults } from '../utils'
import { preparePackages } from '@pnpm/prepare'
import {
  type MutatedProject,
  mutateModules,
} from '@pnpm/core'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

test('should print warnings if set unexpected fields in workspace package manifest', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
    {
      location: 'project-3',
      package: { name: 'project-3' },
    },
  ])
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-3'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
        pnpm: {
          overrides: {},
        },
      },
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
        resolutions: {},
        pnpm: {},
      },
      rootDir: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-3',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/foobar': '100.0.0',
        },
        pnpm: {
          patchedDependencies: {},
        },
      },
      rootDir: path.resolve('project-3'),
    },
  ]
  const opts = await testDefaults({ allProjects, hoistPattern: '*' })
  await mutateModules(importers, {
    ...opts,
    lockfileDir: path.resolve('project-3'),
  })

  expect(logger.warn).toBeCalledTimes(3)
  expect(logger.warn).toHaveBeenCalledWith({ prefix: path.resolve('project-1'), message: `"pnpm" was found in ${path.resolve('project-1')}/package.json. It will not take effect, should configure "pnpm" at the root of the project.` })
  expect(logger.warn).toHaveBeenCalledWith({ prefix: path.resolve('project-2'), message: `"pnpm" was found in ${path.resolve('project-2')}/package.json. It will not take effect, should configure "pnpm" at the root of the project.` })
  expect(logger.warn).toHaveBeenCalledWith({ prefix: path.resolve('project-2'), message: `"resolutions" was found in ${path.resolve('project-2')}/package.json. It will not take effect, should configure "resolutions" at the root of the project.` })
})
