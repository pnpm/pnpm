import path from 'path'
import { add } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'

// This must be a function because some of its values depend on CWD
const createOptions = () => ({
  ...DEFAULT_OPTS,
  rawConfig: {
    ...DEFAULT_OPTS.rawConfig,
    'save-catalog': true,
  },
  registries: {
    ...DEFAULT_OPTS.registries,
  },
  saveCatalog: true,
  dir: process.cwd(),
  cacheDir: path.resolve('cache'),
  storeDir: path.resolve('store'),
  workspaceDir: process.cwd(),
  recursive: true,
} satisfies Partial<add.AddCommandOptions>)

test('saveCatalog creates new workspace manifest with the new catalogs', async () => {
  const project = prepare({
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['@pnpm.e2e/foo'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
    },
  })

  expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/foo': '^2.0.0',
    },
  })

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '2.0.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/foo@2.0.0': {
        resolution: expect.anything(),
      },
    },
  } as Partial<LockfileFile>)
})
