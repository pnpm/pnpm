import path from 'path'
import { sync as writeYamlFile } from 'write-yaml-file'
import type { Config } from '@pnpm/config'
import { preparePackages } from '@pnpm/prepare'
import type { WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { addDistTag } from '@pnpm/registry-mock'
import { execPnpm } from '../utils/index.js'

// TODO: This should work if the settings are passed through CLI
test.skip('recursive update --latest should update deps with correct specs', async () => {
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['*'],
    packageConfigs: {
      'project-2': { saveExact: true },
      'project-3': { savePrefix: '~' },
    },
  } satisfies Partial<Config> & WorkspaceManifest)

  await execPnpm(['recursive', 'update', '--latest'])

  expect((await import(path.resolve('project-1/package.json'))).dependencies).toStrictEqual({ foo: '^100.1.0' })
  expect((await import(path.resolve('project-2/package.json'))).dependencies).toStrictEqual({ foo: '100.1.0' })
  expect((await import(path.resolve('project-3/package.json'))).dependencies).toStrictEqual({ foo: '~100.1.0' })
})
