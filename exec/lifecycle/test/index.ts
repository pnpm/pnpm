/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  runLifecycleHook,
  runLifecycleHooksConcurrently,
  runPostinstallHooks,
} from '@pnpm/exec.lifecycle'
import { tempDir } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import { fixtures } from '@pnpm/test-fixtures'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import type { ProjectRootDir } from '@pnpm/types'
import isWindows from 'is-windows'

const skipOnWindows = isWindows() ? test.skip : test

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))
const rootModulesDir = path.join(import.meta.dirname, '..', 'node_modules')

test('makeNodeRequireOption() preserves existing NODE_OPTIONS', () => {
  expect(makeNodeRequireOption('/project/.pnp.cjs', {
    NODE_OPTIONS: '--max-old-space-size=4096',
  })).toStrictEqual({
    NODE_OPTIONS: '--max-old-space-size=4096 --require=/project/.pnp.cjs',
  })
})

test('makeNodeRequireOption() quotes and escapes module paths with backslashes or whitespace', () => {
  expect(makeNodeRequireOption('C:\\project\\.pnp.cjs', {
    NODE_OPTIONS: '',
  })).toStrictEqual({
    NODE_OPTIONS: '--require="C:\\\\project\\\\.pnp.cjs"',
  })
  expect(makeNodeRequireOption('/project with space/.pnp.cjs', {
    NODE_OPTIONS: '',
  })).toStrictEqual({
    NODE_OPTIONS: '--require="/project with space/.pnp.cjs"',
  })
})

test('makeNodeRequireOption() falls back to NODE_OPTIONS from process.env', () => {
  const nodeOptions = process.env.NODE_OPTIONS
  process.env.NODE_OPTIONS = '--trace-warnings'
  try {
    expect(makeNodeRequireOption('/project/.pnp.cjs', {})).toStrictEqual({
      NODE_OPTIONS: '--trace-warnings --require=/project/.pnp.cjs',
    })
  } finally {
    if (nodeOptions == null) {
      delete process.env.NODE_OPTIONS
    } else {
      process.env.NODE_OPTIONS = nodeOptions
    }
  }
})

test('makeNodePackageMapOption() appends to NODE_OPTIONS', () => {
  expect(makeNodePackageMapOption('/project/node_modules/.package-map.json', {
    NODE_OPTIONS: '--max-old-space-size=4096',
  })).toStrictEqual({
    NODE_OPTIONS: '--max-old-space-size=4096 --experimental-package-map=/project/node_modules/.package-map.json',
  })
})

test('makeNodePackageMapOption() quotes paths with whitespace', () => {
  expect(makeNodePackageMapOption('/project with space/node_modules/.package-map.json', {
    NODE_OPTIONS: '',
  })).toStrictEqual({
    NODE_OPTIONS: '--experimental-package-map="/project with space/node_modules/.package-map.json"',
  })
})

test('makeNodePackageMapOption() quotes and escapes paths with backslashes or quotes', () => {
  expect(makeNodePackageMapOption('C:\\project\\node_modules\\.package-map.json', {
    NODE_OPTIONS: '',
  })).toStrictEqual({
    NODE_OPTIONS: '--experimental-package-map="C:\\\\project\\\\node_modules\\\\.package-map.json"',
  })
  expect(makeNodePackageMapOption('/quo"te/.package-map.json', {
    NODE_OPTIONS: '',
  })).toStrictEqual({
    NODE_OPTIONS: '--experimental-package-map="/quo\\"te/.package-map.json"',
  })
})

test('makeNodePackageMapOption() replaces existing package-map option', () => {
  const nodeOptions = [
    '--experimental-package-map=/old/node_modules/.package-map.json',
    '--max-old-space-size=4096',
    '--experimental-package-map="/old project/.package-map.json"',
    '--experimental-package-map /other/.package-map.json',
    '--inspect',
  ].join(' ')

  expect(makeNodePackageMapOption('/new/node_modules/.package-map.json', {
    NODE_OPTIONS: nodeOptions,
  })).toStrictEqual({
    NODE_OPTIONS: '--max-old-space-size=4096 --inspect --experimental-package-map=/new/node_modules/.package-map.json',
  })
})

test('makeNodePackageMapOption() replaces an existing flag whose path contains an escaped quote', () => {
  expect(makeNodePackageMapOption('/new/.package-map.json', {
    NODE_OPTIONS: '--experimental-package-map="/quo\\"te/old.json" --inspect',
  })).toStrictEqual({
    NODE_OPTIONS: '--inspect --experimental-package-map=/new/.package-map.json',
  })
})

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

test('runLifecycleHook() does not set npm_config env vars but preserves user-defined ones', async () => {
  const pkgRoot = f.find('inspect-npm-config-env')
  await using server = await createTestIpcServer(path.join(pkgRoot, 'test.sock'))
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  const prevPlatformArch = process.env.npm_config_platform_arch
  process.env.npm_config_platform_arch = 'x64'
  try {
    await runLifecycleHook('postinstall', pkg, {
      depPath: '/inspect-npm-config-env/1.0.0',
      pkgRoot,
      rootModulesDir,
      unsafePerm: true,
    })
  } finally {
    if (prevPlatformArch === undefined) {
      delete process.env.npm_config_platform_arch
    } else {
      process.env.npm_config_platform_arch = prevPlatformArch
    }
  }

  expect(server.getLines()).toStrictEqual(['npm_config_platform_arch=x64'])
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
