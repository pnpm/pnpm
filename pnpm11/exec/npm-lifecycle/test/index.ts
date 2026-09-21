import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { lifecycle, type LifecycleChildProcess, type LifecycleLog, makeEnv, relaySignals } from '@pnpm/exec.npm-lifecycle'
import { temporaryDirectory } from 'tempy'

const fixtures = path.join(import.meta.dirname, 'fixtures')
const countTo10 = path.join(fixtures, 'count-to-10')
const countTo10Manifest = readManifest(countTo10)
const originalKill = process.kill
const skipOnWindows = process.platform === 'win32' ? test.skip : test

afterEach(() => {
  process.kill = originalKill
})

skipOnWindows('runs scripts from .hooks directory even if no script is present in package.json', async () => {
  const fixture = temporaryDirectory()
  const dir = path.join(fixture, 'node_modules')
  fs.mkdirSync(path.join(dir, '.hooks'), { recursive: true })
  fs.writeFileSync(path.join(dir, '.hooks', 'postinstall'), '#!/usr/bin/env node\nconsole.log("ran hook")\n', { mode: 0o755 })
  const log = makeLog()

  await lifecycle({ name: 'has-hooks', version: '1.0.0' }, 'postinstall', fixture, {
    stdio: 'pipe',
    log,
    dir,
  })

  expect(log.verbose).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'stdout', expect.stringContaining('ran hook'))
})

test("reports child's output", async () => {
  const log = makeLog()

  await lifecycle(countTo10Manifest, 'postinstall', countTo10, {
    stdio: 'pipe',
    log,
    dir: path.join(import.meta.dirname, '..'),
  })

  expect(log.verbose).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'stdout', expect.stringContaining('line 1'))
  expect(log.verbose).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'stdout', expect.stringContaining('line 2'))
  expect(log.verbose).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'stderr', expect.stringContaining('some error'))
  expect(log.verbose).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'stdout', expect.stringContaining('package.json'))
  expect(log.silly).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'Returned: code:', 0, ' signal:', null)
})

test('runs a script with inherited output', async () => {
  const log = makeLog()

  await lifecycle(countTo10Manifest, 'postinstall', countTo10, {
    stdio: 'inherit',
    log,
    dir: countTo10,
  })

  expect(log.silly).toHaveBeenCalledWith('lifecycle', 'undefined~postinstall:', 'Returned: code:', 0, ' signal:', null)
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

test('the shell emulator settles before a recursive interrupt is raised', async () => {
  const listenersBeforeRelay = new Set(process.listeners('SIGINT'))
  const relay = relaySignals({ kill: () => true }, {
    ownProcessGroup: false,
    raiseOnInterrupt: true,
    terminateOnExit: false,
  })
  const interrupt = process.listeners('SIGINT').find((listener) => !listenersBeforeRelay.has(listener))!
  const raised: Array<[number, string | number | undefined]> = []
  process.kill = ((pid, signal) => {
    raised.push([pid, signal])
    return true
  }) as typeof process.kill
  try {
    const running = lifecycle({ scripts: { test: 'node -e "setTimeout(() => {}, 100)"' } }, 'test', countTo10, {
      stdio: 'pipe',
      log: makeLog(),
      dir: countTo10,
      raiseOnInterrupt: true,
      shellEmulator: true,
    })
    interrupt('SIGINT')
    const settling = relay.settle()
    await new Promise<void>((resolve) => setTimeout(resolve, 20))
    expect(raised).toStrictEqual([])

    await Promise.all([running, settling])
    expect(raised).toStrictEqual([[process.pid, 'SIGINT']])
  } finally {
    await relay.settle()
  }
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
  process.kill = () => true
  const log = makeLog()

  await expect(
    lifecycle(countTo10Manifest, 'signal-int', countTo10, {
      stdio: 'pipe',
      log,
      dir: countTo10,
    })
  ).rejects.toThrow()

  expect(log.info).toHaveBeenCalledWith('lifecycle', 'undefined~signal-int:', 'Failed to exec signal-int script')
  expect(log.silly).toHaveBeenCalledWith('lifecycle', 'undefined~signal-int:', 'Returned: code:', null, ' signal:', 'SIGINT')
})

test('makeEnv', () => {
  const pkg = {
    name: 'myPackage',
    version: '1.0.0',
    contributors: [{ name: 'Mike Sherov', email: 'beep@boop.com' }],
  }
  const privateSettings = [
    'npm_config__auth',
    'npm_config__authToken',
    'npm_config__password',
    'npm_config_//registry.npmjs.org/:_authToken',
    'npm_config_//registry.npmjs.org/:@scope:_authToken',
    'npm_config_@scope:registry',
    'pnpm_config__authToken',
    'pnpm_config_//registry.npmjs.org/:_authToken',
    'NPM_CONFIG__AUTH',
    'NPM_CONFIG__AUTHTOKEN',
    'NPM_CONFIG_//REGISTRY.NPMJS.ORG/:_AUTHTOKEN',
    'PNPM_CONFIG__AUTHTOKEN',
  ]

  process.env.npm_config_platform_arch = 'x64'
  for (const setting of privateSettings) {
    process.env[setting] = 'should-not-leak'
  }
  try {
    const env = makeEnv(pkg, {
      nodeOptions: '--inspect-brk --abort-on-uncaught-exception',
    })
    expect(env.npm_config_platform_arch).toBe('x64')
    for (const setting of privateSettings) {
      expect(env[setting]).toBeUndefined()
    }
    expect(env.npm_package_name).toBe('myPackage')
    expect(env.NODE_OPTIONS).toBe('--inspect-brk --abort-on-uncaught-exception')
  } finally {
    delete process.env.npm_config_platform_arch
    for (const setting of privateSettings) {
      delete process.env[setting]
    }
  }
})

function readManifest (dir: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
}

function makeLog (): LifecycleLog & { [name in 'info' | 'silly' | 'verbose']: jest.Mock<(...args: unknown[]) => void> } {
  return {
    level: 'silent',
    info: jest.fn(),
    warn: jest.fn(),
    silly: jest.fn(),
    verbose: jest.fn(),
    pause: jest.fn(),
    resume: jest.fn(),
  }
}
