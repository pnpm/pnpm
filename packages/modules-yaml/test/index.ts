/// <reference path="../../../typings/index.d.ts"/>
import { read, write } from '@pnpm/modules-yaml'
import readYamlFile from 'read-yaml-file'
import path = require('path')
import isWindows = require('is-windows')
import test = require('tape')
import tempy = require('tempy')

test('write() and read()', async (t) => {
  const modulesDir = tempy.directory()
  const modulesYaml = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
  }
  await write(modulesDir, modulesYaml)
  t.deepEqual(await read(modulesDir), modulesYaml)

  const raw = await readYamlFile(path.join(modulesDir, '.modules.yaml'))
  t.ok(raw['virtualStoreDir'])
  t.equal(path.isAbsolute(raw['virtualStoreDir']), isWindows())

  t.end()
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=true', async (t) => {
  const modulesYaml = await read(path.join(__dirname, 'fixtures/old-shamefully-hoist'))
  t.deepEqual(modulesYaml.publicHoistPattern, ['*'])
  t.deepEqual(modulesYaml.hoistedDependencies, {
    '/accepts/1.3.7': { accepts: 'public' },
    '/array-flatten/1.1.1': { 'array-flatten': 'public' },
    '/body-parser/1.19.0': { 'body-parser': 'public' },
  })
  t.end()
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=false', async (t) => {
  const modulesYaml = await read(path.join(__dirname, 'fixtures/old-no-shamefully-hoist'))
  t.deepEqual(modulesYaml.publicHoistPattern, [])
  t.deepEqual(modulesYaml.hoistedDependencies, {
    '/accepts/1.3.7': { accepts: 'private' },
    '/array-flatten/1.1.1': { 'array-flatten': 'private' },
    '/body-parser/1.19.0': { 'body-parser': 'private' },
  })
  t.end()
})
