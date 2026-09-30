import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'

jest.unstable_mockModule('is-windows', () => ({ default: () => true }))

const { runLifecycleHook } = await import('../lib/index.js')

const testFixtures = fixtures(path.join(import.meta.dirname, 'fixtures'))
const rootModulesDir = path.join(import.meta.dirname, '..', 'node_modules')

// `is-windows` is mocked because the branch under test is otherwise
// reachable only from a Windows host.
test('runLifecycleHook() quotes arguments for the emulator rather than for cmd on Windows', async () => {
  const pkgRoot = testFixtures.prepare('escape-args')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  const args = [
    'C:\\Program Files\\tool\\',
    '',
    'a"b',
    "it's",
    '$PNPM_QUOTING_TEST',
    'line\nbreak',
  ]

  await runLifecycleHook('echo', pkg, {
    args,
    depPath: '/escape-args/1.0.0',
    extraEnv: { PNPM_QUOTING_TEST: 'expanded' },
    pkgRoot,
    rootModulesDir,
    shellEmulator: true,
    unsafePerm: true,
  })

  const recorded = JSON.parse(await fs.promises.readFile(path.join(pkgRoot, 'output.json'), 'utf8'))
  expect(recorded).toStrictEqual(args)
})

// cmd's quoting would leave `$PNPM_QUOTING_TEST` for sh to expand,
// so the recorded arguments show which shell the quoting was chosen for.
// The mocked `is-windows` already runs the Windows quoting branch here.
// A real Windows host has no POSIX shell at a fixed path to configure.
const skipOnRealWindows = process.platform === 'win32' ? test.skip : test

skipOnRealWindows.each([false, true])('runLifecycleHook() quotes arguments for a configured non-cmd scriptShell on Windows (shellEmulator: %s)', async (shellEmulator) => {
  const pkgRoot = testFixtures.prepare('escape-args')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  const args = [
    'C:\\Program Files\\tool\\',
    'a"b',
    "it's",
    '$PNPM_QUOTING_TEST',
  ]

  await runLifecycleHook('echo', pkg, {
    args,
    depPath: '/escape-args/1.0.0',
    extraEnv: { PNPM_QUOTING_TEST: 'expanded' },
    pkgRoot,
    rootModulesDir,
    scriptShell: '/bin/sh',
    shellEmulator,
    unsafePerm: true,
  })

  const recorded = JSON.parse(await fs.promises.readFile(path.join(pkgRoot, 'output.json'), 'utf8'))
  expect(recorded).toStrictEqual(args)
})

test('runLifecycleHook() shows the arguments quoted the POSIX way when cmd quoting runs', async () => {
  const pkgRoot = testFixtures.prepare('escape-args')

  await expect(runLifecycleHook('fail', { name: 'fail', version: '1.0.0', scripts: { fail: 'node -e "process.exit(1)"' } }, {
    args: ['a b', '%PATH%'],
    depPath: '/fail/1.0.0',
    pkgRoot,
    rootModulesDir,
    unsafePerm: true,
  })).rejects.toThrow('fail@1.0.0 fail: `node -e "process.exit(1)" \'a b\' %PATH%`')
})
