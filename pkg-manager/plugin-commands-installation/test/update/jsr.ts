import path from 'path'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { install, update } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { type ProjectManifest } from '@pnpm/types'
import { sync as loadJsonFile } from 'load-json-file'
import { DEFAULT_OPTS } from '../utils'

// This must be a function because some of its values depend on CWD
const createOptions = (jsr: string = DEFAULT_OPTS.registry) => ({
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

test('jsr without alias', async () => {
  await addDistTag({ package: '@jsr/pnpm-e2e__bar', version: '2.0.0', distTag: 'latest' })

  const project = prepare({
    dependencies: {
      '@pnpm-e2e/bar': 'jsr:1.0.0',
    },
  })

  await install.handler(createOptions())
  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm-e2e/bar': {
            specifier: 'jsr:1.0.0',
            version: '@jsr/pnpm-e2e__bar@1.0.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__bar@1.0.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__bar@1.0.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)

  await update.handler({
    ...createOptions(),
    latest: true,
  })
  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      '@pnpm-e2e/bar': 'jsr:2.0.0',
    },
  } as Partial<ProjectManifest>)
  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          '@pnpm-e2e/bar': {
            specifier: 'jsr:2.0.0',
            version: '@jsr/pnpm-e2e__bar@2.0.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__bar@2.0.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__bar@2.0.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})

test('jsr with alias', async () => {
  await addDistTag({ package: '@jsr/pnpm-e2e__bar', version: '2.0.0', distTag: 'latest' })

  const project = prepare({
    dependencies: {
      'bar-from-jsr': 'jsr:@pnpm-e2e/bar@1.0.0',
    },
  })

  await install.handler(createOptions())
  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          'bar-from-jsr': {
            specifier: 'jsr:@pnpm-e2e/bar@1.0.0',
            version: '@jsr/pnpm-e2e__bar@1.0.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__bar@1.0.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__bar@1.0.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)

  await update.handler({
    ...createOptions(),
    latest: true,
  })
  expect(loadJsonFile('package.json')).toMatchObject({
    dependencies: {
      'bar-from-jsr': 'jsr:@pnpm-e2e/bar@2.0.0',
    },
  } as Partial<ProjectManifest>)
  expect(project.readLockfile()).toMatchObject({
    importers: {
      '.': {
        dependencies: {
          'bar-from-jsr': {
            specifier: 'jsr:@pnpm-e2e/bar@2.0.0',
            version: '@jsr/pnpm-e2e__bar@2.0.0',
          },
        },
      },
    },
    packages: {
      '@jsr/pnpm-e2e__bar@2.0.0': {
        resolution: {
          integrity: expect.any(String),
        },
      },
    },
    snapshots: {
      '@jsr/pnpm-e2e__bar@2.0.0': expect.any(Object),
    },
  } as Partial<LockfileFile>)
})
