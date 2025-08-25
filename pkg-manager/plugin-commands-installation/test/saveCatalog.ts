import fs from 'fs'
import path from 'path'
import { add } from '@pnpm/plugin-commands-installation'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { loadJsonFileSync } from 'load-json-file'
import { sync as readYamlFile } from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils/index.js'

// This must be a function because some of its values depend on CWD
const createOptions = (saveCatalogName = 'default'): add.AddCommandOptions => ({
  ...DEFAULT_OPTS,
  saveCatalogName,
  dir: process.cwd(),
  cacheDir: path.resolve('cache'),
  storeDir: path.resolve('store'),
})

test('saveCatalogName creates new workspace manifest with the new catalogs', async () => {
  const project = prepare({
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await add.handler(createOptions(), ['@pnpm.e2e/foo'])

  expect(loadJsonFileSync('package.json')).toHaveProperty(['dependencies'], {
    '@pnpm.e2e/foo': 'catalog:',
  })

  expect(readYamlFile('pnpm-workspace.yaml')).toHaveProperty(['catalog'], {
    '@pnpm.e2e/foo': '^100.1.0',
  })

  expect(project.readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
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
  } as Partial<LockfileFile>))
})

test('saveCatalogName works with different protocols', async () => {
  const project = prepare({
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
  })

  const options = createOptions()
  options.registries['@jsr'] = options.rawConfig['@jsr:registry'] = 'https://npm.jsr.io/'
  await add.handler(options, [
    '@pnpm.e2e/foo@100.1.0',
    'jsr:@rus/greet@0.0.3',
    'github:kevva/is-positive#97edff6',
  ])

  expect(loadJsonFileSync('package.json')).toHaveProperty(['dependencies'], {
    '@pnpm.e2e/foo': 'catalog:',
    '@rus/greet': 'catalog:',
    'is-positive': 'catalog:',
  })

  expect(readYamlFile('pnpm-workspace.yaml')).toHaveProperty(['catalog'], {
    '@pnpm.e2e/foo': '100.1.0',
    '@rus/greet': 'jsr:0.0.3',
    'is-positive': 'github:kevva/is-positive#97edff6',
  })

  expect(project.readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        '@pnpm.e2e/foo': {
          specifier: '100.1.0',
          version: '100.1.0',
        },
        '@rus/greet': {
          specifier: 'jsr:0.0.3',
          version: '0.0.3',
        },
        'is-positive': {
          specifier: 'github:kevva/is-positive#97edff6',
          version: '3.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:',
            version: '100.1.0',
          },
          '@rus/greet': {
            specifier: 'catalog:',
            version: '@jsr/rus__greet@0.0.3',
          },
          'is-positive': {
            specifier: 'catalog:',
            version: 'https://codeload.github.com/kevva/is-positive/tar.gz/97edff6',
          },
        },
      },
    },
  } as Partial<LockfileFile>))
})

test('saveCatalogName does not work with local dependencies', async () => {
  preparePackages([
    {
      name: 'local-dep',
      version: '0.1.2-local',
      private: true,
    },
    {
      name: 'main',
      version: '0.0.0',
      private: true,
    },
  ])

  process.chdir('main')

  await add.handler(createOptions(), ['../local-dep'])

  expect(loadJsonFileSync('package.json')).toStrictEqual({
    name: 'main',
    version: '0.0.0',
    private: true,
    dependencies: {
      'local-dep': process.platform === 'win32'
        ? 'link:..\\local-dep'
        : 'link:../local-dep',
    },
  })

  expect(fs.existsSync('pnpm-workspace.yaml')).toBe(false)

  expect(readYamlFile('pnpm-lock.yaml')).not.toHaveProperty(['catalog'])
  expect(readYamlFile('pnpm-lock.yaml')).not.toHaveProperty(['catalogs'])
})

test('saveCatalogName with non-default name', async () => {
  const project = prepare({
    name: 'test-save-catalog',
    version: '0.0.0',
    private: true,
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await add.handler(createOptions('my-catalog'), ['@pnpm.e2e/foo'])

  expect(loadJsonFileSync('package.json')).toHaveProperty(['dependencies'], {
    '@pnpm.e2e/foo': 'catalog:my-catalog',
  })

  expect(readYamlFile('pnpm-workspace.yaml')).toHaveProperty(['catalogs', 'my-catalog'], {
    '@pnpm.e2e/foo': '^100.1.0',
  })

  expect(project.readLockfile()).toStrictEqual(expect.objectContaining({
    catalogs: {
      'my-catalog': {
        '@pnpm.e2e/foo': {
          specifier: '^100.1.0',
          version: '100.1.0',
        },
      },
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/foo': {
            specifier: 'catalog:my-catalog',
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
  } as Partial<LockfileFile>))
})
