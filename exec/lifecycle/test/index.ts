/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { runLifecycleHook, runPostinstallHooks } from '@pnpm/lifecycle'
import loadJsonFile from 'load-json-file'
import rimraf from '@zkochan/rimraf'

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

test('runLifecycleHook() escapes the args passed to the script', async () => {
  const pkgRoot = path.join(fixtures, 'escape-args')
  const pkg = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('echo', pkg, {
    depPath: '/escape-args/1.0.0',
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
    args: ['Revert "feature (#1)"'],
  })

  expect((await import(path.join(pkgRoot, 'output.json'))).default).toStrictEqual(['Revert "feature (#1)"'])
})

test('runPostinstallHooks()', async () => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  await rimraf(path.join(pkgRoot, 'output.json'))
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
