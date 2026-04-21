/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import { runLifecycleHook, runLifecycleHooksConcurrently, runPostinstallHooks } from '@pnpm/exec.lifecycle'
import { tempDir } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import type { ProjectRootDir } from '@pnpm/types'
import isWindows from 'is-windows'

const skipOnWindows = isWindows() ? test.skip : test

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))
const rootModulesDir = path.join(import.meta.dirname, '..', 'node_modules')

test('runLifecycleHook()', async () => {
  const pkgRoot = f.find('simple')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    depPath: '/simple/1.0.0',
    optional: false,
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['install'])
})

test('runLifecycleHook() escapes the args passed to the script', async () => {
  const pkgRoot = f.find('escape-args')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('echo', pkg, {
    depPath: '/escape-args/1.0.0',
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
    args: ['Revert "feature (#1)"'],
  })

  expect((await import(path.join(pkgRoot, 'output.json'))).default).toStrictEqual(['Revert "feature (#1)"'])
})

test('runLifecycleHook() passes newline correctly', async () => {
  const pkgRoot = f.find('escape-newline')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('echo', pkg, {
    depPath: 'escape-newline@1.0.0',
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
    args: ['a\nb != \'A\\nB\''],
  })

  expect((await import(path.join(pkgRoot, 'output.json'))).default).toStrictEqual([
    process.platform === 'win32' ? 'a\\nb != \'A\\\\nB\'' : 'a\nb != \'A\\nB\'',
  ])
})

test('runLifecycleHook() does not set npm_config env vars', async () => {
  const pkgRoot = f.find('inspect-frozen-lockfile')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    depPath: '/inspect-frozen-lockfile/1.0.0',
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['unset'])
})

test('runPostinstallHooks()', async () => {
  const pkgRoot = f.find('with-many-scripts')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  await runPostinstallHooks({
    depPath: '/with-many-scripts/1.0.0',
    optional: false,
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
  })

  expect(server.getLines()).toStrictEqual(['preinstall', 'install', 'postinstall'])
})

test('runLifecycleHook() should throw an error while missing script start or file server.js', async () => {
  const pkgRoot = f.find('without-script-start-serverjs')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  await expect(
    runLifecycleHook('start', pkg, {
      depPath: '/without-script-start-serverjs/1.0.0',
      optional: false,
      pkgRoot,
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
    unsafePerm: true,
  })

  expect(fs.existsSync(path.join(projectDir, 'foo'))).toBeTruthy()
})
