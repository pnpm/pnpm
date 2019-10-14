///<reference path="../../../typings/index.d.ts"/>
import { read, write } from '@pnpm/modules-yaml'
import test = require('tape')
import tempy = require('tempy')

test('write() and read()', async (t) => {
  const modulesYaml = {
    hoistedAliases: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    independentLeaves: false,
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    shamefullyHoist: false,
    skipped: [],
    store: '/.pnpm-store',
    virtualStoreDir: '/src/project/node_modules/.pnpm',
  }
  const tempDir = tempy.directory()
  await write(tempDir, modulesYaml)
  t.deepEqual(await read(tempDir), modulesYaml)
  t.end()
})
