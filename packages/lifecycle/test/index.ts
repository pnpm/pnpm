/// <reference path="../../../typings/index.d.ts"/>
import runLifecycleHook, { runPostinstallHooks } from '@pnpm/lifecycle'
import path = require('path')
import loadJsonFile = require('load-json-file')
import rimraf = require('rimraf')

const fixtures = path.join(__dirname, 'fixtures')
const rootModulesDir = path.join(__dirname, '..', 'node_modules')

test('runLifecycleHook()', async () => {
  const pkgRoot = path.join(fixtures, 'simple')
  const pkg = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    depPath: '/simple/1.0.0',
    optional: false,
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect((await import(path.join(pkgRoot, 'output.json'))).default).toStrictEqual(['install'])
})

test('runPostinstallHooks()', async () => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  rimraf.sync(path.join(pkgRoot, 'output.json'))
  await runPostinstallHooks({
    depPath: '/with-many-scripts/1.0.0',
    optional: false,
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect(loadJsonFile.sync(path.join(pkgRoot, 'output.json'))).toStrictEqual(['preinstall', 'install', 'postinstall'])
})

test('runPostinstallHooks() with prepare = true', async () => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  rimraf.sync(path.join(pkgRoot, 'output.json'))
  await runPostinstallHooks({
    depPath: '/with-many-scripts/1.0.0',
    optional: false,
    pkgRoot,
    prepare: true,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect(loadJsonFile.sync(path.join(pkgRoot, 'output.json'))).toStrictEqual(['preinstall', 'install', 'postinstall', 'prepare'])
})
