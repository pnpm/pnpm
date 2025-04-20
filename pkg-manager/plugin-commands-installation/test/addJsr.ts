import path from 'path'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { add } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { sync as loadJsonFile } from 'load-json-file'
import { DEFAULT_OPTS } from './utils'

// This must be a function because some of its values depend on CWD
const createOptions = (jsr: string = 'https://npm.jsr.io/') => ({
  ...DEFAULT_OPTS,
  rawConfig: {
    ...DEFAULT_OPTS.rawConfig,
    '@jsr:registry': jsr,
  },
  registries: {
    ...DEFAULT_OPTS.registries,
    '@jsr': jsr,
  },
  dir: process.cwd(),
  cacheDir: path.resolve('cache'),
  storeDir: path.resolve('store'),
})

test('pnpm add jsr:@<scope>/<name>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['jsr:@pnpm-e2e/foo'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:^0.1.0',
    },
  } as ProjectManifest)

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm-e2e/foo': {
            specifier: 'jsr:^0.1.0',
            version: '@jsr/pnpm-e2e__foo@0.1.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__foo@0.1.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__foo@0.1.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})

test('pnpm add jsr:@<scope>/<name>@latest', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['jsr:@pnpm-e2e/foo@latest'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:^0.1.0',
    },
  } as ProjectManifest)

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm-e2e/foo': {
            specifier: 'jsr:^0.1.0',
            version: '@jsr/pnpm-e2e__foo@0.1.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__foo@0.1.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__foo@0.1.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})

test('pnpm add jsr:@<scope>/<name>@<version_selector>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['jsr:@pnpm-e2e/foo@0.1'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:~0.1.0',
    },
  } as ProjectManifest)

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm-e2e/foo': {
            specifier: 'jsr:~0.1.0',
            version: '@jsr/pnpm-e2e__foo@0.1.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__foo@0.1.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__foo@0.1.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})

test('pnpm add <alias>@jsr:@<scope>/<name>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['foo-from-jsr@jsr:@pnpm-e2e/foo'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      'foo-from-jsr': 'jsr:@pnpm-e2e/foo@^0.1.0',
    },
  } as ProjectManifest)

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          'foo-from-jsr': {
            specifier: 'jsr:@pnpm-e2e/foo@^0.1.0',
            version: '@jsr/pnpm-e2e__foo@0.1.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__foo@0.1.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__foo@0.1.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})

test('pnpm add <alias>@jsr:@<scope>/<name>@<version_selector>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['foo-from-jsr@jsr:@pnpm-e2e/foo@0.1'])

  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      'foo-from-jsr': 'jsr:@pnpm-e2e/foo@~0.1.0',
    },
  } as ProjectManifest)

  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          'foo-from-jsr': {
            specifier: 'jsr:@pnpm-e2e/foo@~0.1.0',
            version: '@jsr/pnpm-e2e__foo@0.1.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__foo@0.1.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__foo@0.1.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})
