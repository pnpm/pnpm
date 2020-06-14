///<reference path="../../../typings/index.d.ts"/>
import { read, write } from '@pnpm/modules-yaml'
import isWindows = require('is-windows')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import test = require('tape')
import tempy = require('tempy')

test('write() and read()', async (t) => {
  const modulesDir = tempy.directory()
  const modulesYaml = {
    hoistedAliases: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
  }
  await write(modulesDir, modulesYaml)
  delete modulesYaml.hoistedAliases
  t.deepEqual(await read(modulesDir), modulesYaml)

  const raw = await readYamlFile(path.join(modulesDir, '.modules.yaml'))
  t.ok(raw['virtualStoreDir'])
  t.equal(path.isAbsolute(raw['virtualStoreDir']), isWindows())

  t.end()
})

test('backward compatible read', async (t) => {
  const modulesYaml = await read(path.join(__dirname, 'fixtures/old-shamefully-hoist'))
  t.deepEqual(modulesYaml.publicHoistPattern, ['*'])
  t.deepEqual(modulesYaml.publicHoistedAliases, [
    '/accepts/1.3.7',
    '/array-flatten/1.1.1',
    '/body-parser/1.19.0',
  ])
  t.end()
})
