import test = require('tape')
import { read, write, Modules } from '@pnpm/modules-yaml'
import tempy = require('tempy')

test('write() and read()', async (t) => {
  const modulesYaml = {
    importers: {
      '.': {
        hoistedAliases: {},
        shamefullyFlatten: false,
      },
    },
    included: {
      devDependencies: true,
      dependencies: true,
      optionalDependencies: true,
    },
    independentLeaves: false,
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    skipped: [],
    store: '/.pnpm-store',
  }
  const tempDir = tempy.directory()
  await write(tempDir, modulesYaml)
  t.deepEqual(await read(tempDir), modulesYaml)
  t.end()
})
