/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { readModulesManifest, writeModulesManifest, type StrictModules } from '@pnpm/modules-yaml'
import { sync as readYamlFile } from 'read-yaml-file'
import isWindows from 'is-windows'
import { temporaryDirectory } from 'tempy'

test('writeModulesManifest() and readModulesManifest()', async () => {
  const modulesDir = temporaryDirectory()
  const modulesYaml: StrictModules = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    ignoredBuilds: new Set(),
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    prunedAt: new Date().toUTCString(),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, modulesYaml)
  expect(await readModulesManifest(modulesDir)).toEqual(modulesYaml)

  const raw = readYamlFile<any>(path.join(modulesDir, '.modules.yaml')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(raw.virtualStoreDir).toBeDefined()
  expect(path.isAbsolute(raw.virtualStoreDir)).toEqual(isWindows())
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=true', async () => {
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/old-shamefully-hoist'))
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
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/old-no-shamefully-hoist'))
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

test('readModulesManifest() should create a node_modules directory', async () => {
  const modulesDir = path.join(temporaryDirectory(), 'node_modules')
  const modulesYaml: StrictModules = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    ignoredBuilds: new Set(),
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    prunedAt: new Date().toUTCString(),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, modulesYaml)
  expect(await readModulesManifest(modulesDir)).toEqual(modulesYaml)
})

test('readModulesManifest does not fail on empty file', async () => {
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/empty-modules-yaml'))
  expect(modulesYaml).toBeUndefined()
})
