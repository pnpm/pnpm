/// <reference path="../../../typings/index.d.ts"/>
import { read, write } from '@pnpm/modules-yaml'
import readYamlFile from 'read-yaml-file'
import path = require('path')
import isWindows = require('is-windows')
import tempy = require('tempy')

test('write() and read()', async () => {
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
  expect(await read(modulesDir)).toEqual(modulesYaml)

  const raw = await readYamlFile<object>(path.join(modulesDir, '.modules.yaml'))
  expect(raw['virtualStoreDir']).toBeDefined()
  expect(path.isAbsolute(raw['virtualStoreDir'])).toEqual(isWindows())
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=true', async () => {
  const modulesYaml = await read(path.join(__dirname, 'fixtures/old-shamefully-hoist'))
  if (modulesYaml == null) {
    fail('modulesYaml was nullish')
  }
  expect(modulesYaml.publicHoistPattern).toEqual(['*'])
  expect(modulesYaml.hoistedDependencies).toEqual({
    '/accepts/1.3.7': { accepts: 'public' },
    '/array-flatten/1.1.1': { 'array-flatten': 'public' },
    '/body-parser/1.19.0': { 'body-parser': 'public' },
  })
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=false', async () => {
  const modulesYaml = await read(path.join(__dirname, 'fixtures/old-no-shamefully-hoist'))
  if (modulesYaml == null) {
    fail('modulesYaml was nullish')
  }
  expect(modulesYaml.publicHoistPattern).toEqual([])
  expect(modulesYaml.hoistedDependencies).toEqual({
    '/accepts/1.3.7': { accepts: 'private' },
    '/array-flatten/1.1.1': { 'array-flatten': 'private' },
    '/body-parser/1.19.0': { 'body-parser': 'private' },
  })
})
