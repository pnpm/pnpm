/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { runLifecycleHook, runLifecycleHooksConcurrently, runPostinstallHooks } from '@pnpm/lifecycle'
import { PnpmError } from '@pnpm/error'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { fixtures } from '@pnpm/test-fixtures'
import { tempDir } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { type StoreController } from '@pnpm/store-controller-types'
import isWindows from 'is-windows'

const skipOnWindows = isWindows() ? test.skip : test

const f = fixtures(path.join(__dirname, 'fixtures'))
const rootModulesDir = path.join(__dirname, '..', 'node_modules')

test('runLifecycleHook()', async () => {
  const pkgRoot = f.find('simple')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  const pkg = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    depPath: '/simple/1.0.0',
    optional: false,
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['install'])
})

test('runLifecycleHook() escapes the args passed to the script', async () => {
  const pkgRoot = f.find('escape-args')
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

test('runLifecycleHook() sets frozen-lockfile to false', async () => {
  const pkgRoot = f.find('inspect-frozen-lockfile')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  const pkg = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    depPath: '/inspect-frozen-lockfile/1.0.0',
    pkgRoot,
    rawConfig: {
      'frozen-lockfile': true,
    },
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['empty string'])
})

test('runPostinstallHooks()', async () => {
  const pkgRoot = f.find('with-many-scripts')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  await runPostinstallHooks({
    depPath: '/with-many-scripts/1.0.0',
    optional: false,
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall'])
})

test('runLifecycleHook() should throw an error while missing script start or file server.js', async () => {
  const pkgRoot = f.find('without-script-start-serverjs')
  const pkg = await import(path.join(pkgRoot, 'package.json'))
  await expect(
    runLifecycleHook('start', pkg, {
      depPath: '/without-script-start-serverjs/1.0.0',
      optional: false,
      pkgRoot,
      rawConfig: {},
      rootModulesDir,
      unsafePerm: true,
    })
  ).rejects.toThrow(new PnpmError('NO_SCRIPT_OR_SERVER', 'Missing script start or file server.js'))
})

test('preinstall script does not trigger node-gyp rebuild', async () => {
  const pkgRoot = f.find('gyp-with-preinstall')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  await runPostinstallHooks({
    depPath: '/gyp-with-preinstall/1.0.0',
    optional: false,
    pkgRoot,
    rawConfig: {},
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['preinstall'])
})

skipOnWindows('runLifecycleHooksConcurrently() should check binding.gyp', async () => {
  const projectDir = tempDir(false)

  fs.writeFileSync(path.join(projectDir, 'binding.gyp'), JSON.stringify({
    targets: [
      {
        target_name: 'run_js_script',
        actions: [
          {
            action_name: 'execute_postinstall',
            inputs: [],
            outputs: ['foo'],
            action: ['node', '-e', 'require(\'fs\').writeFileSync(\'foo\', \'\', \'utf8\')'],
          },
        ],
      },
    ],
  }), 'utf8')

  await runLifecycleHooksConcurrently(['install'], [{ buildIndex: 0, rootDir: projectDir as ProjectRootDir, modulesDir: '', manifest: {} }], 5, {
    storeController: {} as StoreController,
    optional: false,
    rawConfig: {},
    unsafePerm: true,
  })

  expect(fs.existsSync(path.join(projectDir, 'foo'))).toBeTruthy()
})
