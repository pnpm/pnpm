import path from 'path'
import { add } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'

// This must be a function because some of its values depend on CWD
const createOptions = (): add.AddCommandOptions => ({
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
})

test('saveCatalog creates new workspace manifest with the new catalogs', async () => {
  const project = prepare({
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await add.handler(createOptions(), ['@pnpm.e2e/foo'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
    },
  })

  expect(readYamlFile('pnpm-workspace.yaml')).toStrictEqual({
    catalog: {
      '@pnpm.e2e/foo': '^100.1.0',
    },
  })

  expect(project.readLockfile()).toMatchObject({
    // catalogs: {
    //   default: {
    //     '@pnpm.e2e/foo': {
    //       specifier: '^100.1.0',
    //       version: '100.1.0',
    //     },
    //   },
    // },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
        },
      },
    },
    packages: {
      '@pnpm.e2e/foo@100.1.0': {
        resolution: expect.anything(),
      },
    },
  } as Partial<LockfileFile>)
})
