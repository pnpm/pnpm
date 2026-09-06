import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'

jest.unstable_mockModule('is-windows', () => ({ default: () => true }))

const { runLifecycleHook } = await import('../lib/index.js')

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))
const rootModulesDir = path.join(import.meta.dirname, '..', 'node_modules')

// The emulator is a POSIX shell on every platform, so a Windows host must
// still get shlex quoting rather than the JSON quoting `cmd` needs. Mocking
// `is-windows` is what keeps this branch reachable off Windows.
test('runLifecycleHook() quotes arguments for the emulator rather than for cmd on Windows', async () => {
  const pkgRoot = f.prepare('escape-args')
  const { default: pkg } = await import(path.join(pkgRoot, 'package.json'))
  const args = [
    'C:\\Program Files\\tool\\',
    '',
    'a"b',
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
