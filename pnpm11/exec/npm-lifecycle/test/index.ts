import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { lifecycle, type LifecycleChildProcess, type LifecycleLog, makeEnv } from '@pnpm/exec.npm-lifecycle'
import { temporaryDirectory } from 'tempy'

const fixtures = path.join(import.meta.dirname, 'fixtures')
const countTo10 = path.join(fixtures, 'count-to-10')
const countTo10Manifest = readManifest(countTo10)

type Spy = ((...args: unknown[]) => void) & {
  calls: unknown[][]
  calledWithMatch: (...matchers: unknown[]) => boolean
}

function spy (): Spy {
  const calls: unknown[][] = []
  const fn = ((...args: unknown[]) => {
    calls.push(args)
  }) as Spy
  fn.calls = calls
  fn.calledWithMatch = (...matchers) => calls.some(call =>
    matchers.every((m, i) => {
      if (typeof m === 'string') return typeof call[i] === 'string' && (call[i] as string).includes(m)
      return call[i] === m
    })
  )
  return fn
}

function noop (): void {}

function makeLog (overrides: Partial<LifecycleLog> = {}): LifecycleLog {
  return {
    level: 'silent',
    info: noop,
    warn: noop,
    silly: spy(),
    verbose: spy(),
    pause: noop,
    resume: noop,
    ...overrides,
  }
}

function readManifest (dir: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
}

const originalKill = process.kill

afterEach(() => {
  process.kill = originalKill
})

const skipOnWindows = process.platform === 'win32' ? test.skip : test

skipOnWindows('runs scripts from .hooks directory even if no script is present in package.json', async () => {
  const fixture = temporaryDirectory()
  const dir = path.join(fixture, 'node_modules')
  fs.mkdirSync(path.join(dir, '.hooks'), { recursive: true })
  fs.writeFileSync(path.join(dir, '.hooks', 'postinstall'), '#!/usr/bin/env node\nconsole.log("ran hook")\n', { mode: 0o755 })
  const verbose = spy()
  const log = makeLog({ verbose })

  await lifecycle({ name: 'has-hooks', version: '1.0.0' }, 'postinstall', fixture, {
    stdio: 'pipe',
    log,
    dir,
  })

  expect(verbose.calledWithMatch('lifecycle', 'undefined~postinstall:', 'stdout', 'ran hook')).toBe(true)
})

test("reports child's output", async () => {
  const verbose = spy()
  const silly = spy()
  const log = makeLog({ verbose, silly })

  await lifecycle(countTo10Manifest, 'postinstall', countTo10, {
    stdio: 'pipe',
    log,
    dir: path.join(import.meta.dirname, '..'),
  })

  expect(verbose.calledWithMatch('lifecycle', 'undefined~postinstall:', 'stdout', 'line 1')).toBe(true)
  expect(verbose.calledWithMatch('lifecycle', 'undefined~postinstall:', 'stdout', 'line 2')).toBe(true)
  expect(verbose.calledWithMatch('lifecycle', 'undefined~postinstall:', 'stderr', 'some error')).toBe(true)
  expect(verbose.calledWithMatch('lifecycle', 'undefined~postinstall:', 'stdout', 'package.json')).toBe(true)
  expect(silly.calledWithMatch('lifecycle', 'undefined~postinstall:', 'Returned: code:', 0, ' signal:', null)).toBe(true)
})

test('reports the spawned lifecycle process', async () => {
  let spawned: LifecycleChildProcess | undefined

  await lifecycle(countTo10Manifest, 'postinstall', countTo10, {
    stdio: 'pipe',
    log: makeLog(),
    dir: countTo10,
    onSpawn: child => {
      spawned = child
    },
  })

  expect(typeof spawned!.pid).toBe('number')
  expect(typeof spawned!.once).toBe('function')
  expect(typeof spawned!.kill).toBe('function')
})

test('rejects when the spawn observer fails', async () => {
  let childClosed = false

  await expect(
    lifecycle(countTo10Manifest, 'postinstall', countTo10, {
      stdio: 'pipe',
      log: makeLog(),
      dir: countTo10,
      onSpawn: child => {
        child.once('close', () => {
          childClosed = true
        })
        throw new Error('observer failed')
      },
    })
  ).rejects.toThrow(/observer failed/)
  expect(childClosed).toBe(true)
})

test('runs lifecycle scripts with the shell emulator', async () => {
  let spawned = false

  await lifecycle(countTo10Manifest, 'postinstall', countTo10, {
    stdio: 'pipe',
    log: makeLog(),
    dir: countTo10,
    shellEmulator: true,
    onSpawn: () => {
      spawned = true
    },
  })

  expect(spawned).toBe(false)
})

test('a script killed by a signal rejects', async () => {
  process.kill = () => true

  await expect(
    lifecycle(countTo10Manifest, 'signal-abrt', countTo10, {
      stdio: 'pipe',
      log: makeLog(),
      dir: countTo10,
    })
  ).rejects.toThrow()
})

skipOnWindows('exit with error on INT signal from child', async () => {
  const verbose = spy()
  const silly = spy()
  const info = spy()
  process.kill = () => true
  const log = makeLog({ verbose, silly, info })

  await expect(
    lifecycle(countTo10Manifest, 'signal-int', countTo10, {
      stdio: 'pipe',
      log,
      dir: countTo10,
    })
  ).rejects.toThrow()

  expect(info.calledWithMatch('lifecycle', 'undefined~signal-int:', 'Failed to exec signal-int script')).toBe(true)
  expect(silly.calledWithMatch('lifecycle', 'undefined~signal-int:', 'Returned: code:', null, ' signal:', 'SIGINT')).toBe(true)
})

test('makeEnv', () => {
  const pkg = {
    name: 'myPackage',
    version: '1.0.0',
    contributors: [{ name: 'Mike Sherov', email: 'beep@boop.com' }],
  }

  process.env.npm_config_platform_arch = 'x64'
  process.env.npm_config__auth = 'c2hvdWxkLW5vdC1sZWFr'
  process.env.npm_config__authToken = 'should-not-leak'
  process.env.npm_config__password = 'should-not-leak'
  process.env['npm_config_//registry.npmjs.org/:_authToken'] = 'should-not-leak'
  process.env['npm_config_//registry.npmjs.org/:@scope:_authToken'] = 'should-not-leak'
  process.env['npm_config_@scope:registry'] = 'https://example.com'
  process.env.pnpm_config__authToken = 'should-not-leak'
  process.env['pnpm_config_//registry.npmjs.org/:_authToken'] = 'should-not-leak'
  try {
    const env = makeEnv(pkg, {
      nodeOptions: '--inspect-brk --abort-on-uncaught-exception',
    })
    expect(env.npm_config_platform_arch).toBe('x64')
    expect(env.npm_config__auth).toBeUndefined()
    expect(env.npm_config__authToken).toBeUndefined()
    expect(env.npm_config__password).toBeUndefined()
    expect(env['npm_config_//registry.npmjs.org/:_authToken']).toBeUndefined()
    expect(env['npm_config_//registry.npmjs.org/:@scope:_authToken']).toBeUndefined()
    expect(env['npm_config_@scope:registry']).toBeUndefined()
    expect(env.pnpm_config__authToken).toBeUndefined()
    expect(env['pnpm_config_//registry.npmjs.org/:_authToken']).toBeUndefined()
    expect(env.npm_package_name).toBe('myPackage')
    expect(env.NODE_OPTIONS).toBe('--inspect-brk --abort-on-uncaught-exception')
  } finally {
    delete process.env.npm_config_platform_arch
    delete process.env.npm_config__auth
    delete process.env.npm_config__authToken
    delete process.env.npm_config__password
    delete process.env['npm_config_//registry.npmjs.org/:_authToken']
    delete process.env['npm_config_//registry.npmjs.org/:@scope:_authToken']
    delete process.env['npm_config_@scope:registry']
    delete process.env.pnpm_config__authToken
    delete process.env['pnpm_config_//registry.npmjs.org/:_authToken']
  }
})
