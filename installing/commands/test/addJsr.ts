import path from 'node:path'

import { expect, test } from '@jest/globals'
import { add } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'

import { DEFAULT_OPTS } from './utils/index.js'

// This must be a function because some of its values depend on CWD
const createOptions = (jsr: string = 'https://npm.jsr.io/') => ({
  ...DEFAULT_OPTS,
  configByUri: {},
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

  expect(loadJsonFileSync('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:^0.1.0',
    },
  })

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
  })
})

test('pnpm add jsr:@<scope>/<name> --save-peer writes a valid peer range', async () => {
  prepare()

  await add.handler({
    ...createOptions(),
    savePeer: true,
  }, ['jsr:@pnpm-e2e/foo'])

  expect(loadJsonFileSync('package.json')).toMatchObject({
    devDependencies: {
      '@pnpm-e2e/foo': 'jsr:^0.1.0',
    },
    peerDependencies: {
      '@pnpm-e2e/foo': '^0.1.0',
    },
  })
})

test('pnpm add jsr:@<scope>/<name>@latest', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['jsr:@pnpm-e2e/foo@latest'])

  expect(loadJsonFileSync('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:^0.1.0',
    },
  })

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
  })
})

test('pnpm add jsr:@<scope>/<name>@<version_selector>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['jsr:@pnpm-e2e/foo@0.1'])

  expect(loadJsonFileSync('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/foo': 'jsr:~0.1.0',
    },
  })

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
  })
})

test('pnpm add <alias>@jsr:@<scope>/<name>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['foo-from-jsr@jsr:@pnpm-e2e/foo'])

  expect(loadJsonFileSync('package.json')).toMatchObject({
    dependencies: {
      'foo-from-jsr': 'jsr:@pnpm-e2e/foo@^0.1.0',
    },
  })

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
  })
})

test('pnpm add <alias>@jsr:@<scope>/<name>@<version_selector>', async () => {
  const project = prepare({
    name: 'test-add-jsr',
    version: '0.0.0',
    private: true,
  })

  await add.handler(createOptions(), ['foo-from-jsr@jsr:@pnpm-e2e/foo@0.1'])

  expect(loadJsonFileSync('package.json')).toMatchObject({
    dependencies: {
      'foo-from-jsr': 'jsr:@pnpm-e2e/foo@~0.1.0',
    },
  })

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
  })
})
