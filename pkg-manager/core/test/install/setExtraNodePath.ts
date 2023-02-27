import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import {
  MutatedProject,
  mutateModules,
} from '@pnpm/core'
import { sync as loadJsonFile } from 'load-json-file'
import { testDefaults } from '../utils'

test('jest CLI should print the write version when multiple instances of jest are used in a workspace', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
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
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        scripts: {
          postinstall: 'jest --version | json-append output.json',
        },

        dependencies: {
          jest: '27.5.1',
          'json-append': '1.1.1',
        },
      },
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        scripts: {
          postinstall: 'jest --version | json-append output.json',
        },

        dependencies: {
          jest: '24.9.0',
          'json-append': '1.1.1',
        },
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    extendNodePath: true,
    fastUnpack: false,
    hoistPattern: '@babel/*',
  }))

  {
    const [jestVersion] = loadJsonFile<string[]>('project-1/output.json')
    expect(jestVersion.trim()).toStrictEqual('27.5.1')
  }
  {
    const [jestVersion] = loadJsonFile<string[]>('project-2/output.json')
    expect(jestVersion.trim()).toStrictEqual('24.9.0')
  }
})
