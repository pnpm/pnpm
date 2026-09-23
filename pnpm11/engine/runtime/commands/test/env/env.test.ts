import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'

const mockRunPnpmCli = jest.fn()
jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: mockRunPnpmCli,
}))

const { env } = await import('@pnpm/engine.runtime.commands')

beforeEach(() => {
  mockRunPnpmCli.mockClear()
})

test('env use calls pnpm add with the correct arguments', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    cacheDir: '/tmp/cache',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    configByUri: {},
    storeDir: '/tmp/store',
  }, ['use', '18'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', '--global', 'node@runtime:18', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store', '--cache-dir', '/tmp/cache'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('env use passes lts specifier through unchanged', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    configByUri: {},
    storeDir: '/tmp/store',
  }, ['use', 'lts'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', '--global', 'node@runtime:lts', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('env use passes codename specifier through unchanged', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    configByUri: {},
    storeDir: '/tmp/store',
  }, ['use', 'argon'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', '--global', 'node@runtime:argon', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('fail if not run with --global', async () => {
  await expect(
    env.handler({
      bin: '/usr/local/bin',
      global: false,
      pnpmHomeDir: '/tmp/pnpm-home',
      configByUri: {},
    }, ['use', '18'])
  ).rejects.toEqual(new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('fail if there is no global bin directory', async () => {
  await expect(
    env.handler({
      // @ts-expect-error
      bin: undefined,
      global: true,
      pnpmHomeDir: '/tmp/pnpm-home',
      configByUri: {},
    }, ['use', 'lts'])
  ).rejects.toEqual(new PnpmError('CANNOT_MANAGE_NODE', 'Unable to manage Node.js because pnpm was not installed using the standalone installation script'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('env remove calls pnpm remove when installed node version matches', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
  const pnpmHomeDir = path.join(tempDir, 'home')
  const installDir = path.join(pnpmHomeDir, 'global', 'v11', 'install-node')
  fs.mkdirSync(path.join(installDir, 'node_modules', 'node'), { recursive: true })
  fs.writeFileSync(path.join(installDir, 'node_modules', 'node', 'package.json'), JSON.stringify({ name: 'node', version: '18.12.0' }))
  fs.symlinkSync(installDir, path.join(pnpmHomeDir, 'global', 'v11', 'hash-node'))

  await env.handler({
    bin: '/usr/local/bin',
    cacheDir: '/tmp/cache',
    global: true,
    pnpmHomeDir,
    configByUri: {},
    storeDir: '/tmp/store',
  }, ['remove', '18'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['remove', '--global', 'node', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store', '--cache-dir', '/tmp/cache'],
    { cwd: pnpmHomeDir }
  )

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('env remove does not call pnpm remove when installed node version does not match', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
  const pnpmHomeDir = path.join(tempDir, 'home')
  const installDir = path.join(pnpmHomeDir, 'global', 'v11', 'install-node')
  fs.mkdirSync(path.join(installDir, 'node_modules', 'node'), { recursive: true })
  fs.writeFileSync(path.join(installDir, 'node_modules', 'node', 'package.json'), JSON.stringify({ name: 'node', version: '20.8.0' }))
  fs.symlinkSync(installDir, path.join(pnpmHomeDir, 'global', 'v11', 'hash-node'))

  await expect(
    env.handler({
      bin: '/usr/local/bin',
      global: true,
      pnpmHomeDir,
      configByUri: {},
    }, ['remove', '18'])
  ).rejects.toEqual(new PnpmError('ENV_NO_NODE_DIRECTORY', "Couldn't find Node.js version matching 18"))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('env remove does not delete legacy directories that only share a prefix without boundary', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
  const pnpmHomeDir = path.join(tempDir, 'home')
  const nodejsDir = path.join(pnpmHomeDir, 'nodejs')
  fs.mkdirSync(path.join(nodejsDir, '20.8.0'), { recursive: true })
  fs.mkdirSync(path.join(nodejsDir, '22.0.0'), { recursive: true })

  await expect(
    env.handler({
      bin: '/usr/local/bin',
      global: true,
      pnpmHomeDir,
      configByUri: {},
    }, ['remove', '2'])
  ).rejects.toEqual(new PnpmError('ENV_NO_NODE_DIRECTORY', "Couldn't find Node.js version matching 2"))

  expect(fs.existsSync(path.join(nodejsDir, '20.8.0'))).toBe(true)
  expect(fs.existsSync(path.join(nodejsDir, '22.0.0'))).toBe(true)

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('env remove supports multiple version arguments', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
  const pnpmHomeDir = path.join(tempDir, 'home')
  const nodejsDir = path.join(pnpmHomeDir, 'nodejs')
  fs.mkdirSync(path.join(nodejsDir, '14.0.0'), { recursive: true })
  fs.mkdirSync(path.join(nodejsDir, '16.2.3'), { recursive: true })

  await env.handler({
    bin: '/usr/local/bin',
    global: true,
    pnpmHomeDir,
    configByUri: {},
  }, ['remove', '14.0.0', '16.2.3'])

  expect(fs.existsSync(path.join(nodejsDir, '14.0.0'))).toBe(false)
  expect(fs.existsSync(path.join(nodejsDir, '16.2.3'))).toBe(false)

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('env remove fails if not run with --global', async () => {
  await expect(
    env.handler({
      bin: '/usr/local/bin',
      global: false,
      pnpmHomeDir: '/tmp/pnpm-home',
      configByUri: {},
    }, ['remove', '18'])
  ).rejects.toEqual(new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env remove <version>" can only be used with the "--global" option currently'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('env remove fails if no version is specified', async () => {
  await expect(
    env.handler({
      bin: '/usr/local/bin',
      global: true,
      pnpmHomeDir: '/tmp/pnpm-home',
      configByUri: {},
    }, ['remove'])
  ).rejects.toEqual(new PnpmError('MISSING_NODE_VERSION', '"pnpm env remove --global <version>" requires a Node.js version to be specified'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('env remove cleans up dangling symlink when node is removed (pnpm/pnpm#6122)', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
  const binDir = path.join(tempDir, 'bin')
  const pnpmHomeDir = path.join(tempDir, 'home')
  fs.mkdirSync(binDir, { recursive: true })
  fs.mkdirSync(pnpmHomeDir, { recursive: true })

  const nonExistentTarget = path.join(pnpmHomeDir, 'nodejs', '18.12.1', 'bin', 'node')
  const binNode = path.join(binDir, 'node')
  fs.symlinkSync(nonExistentTarget, binNode)

  expect(fs.lstatSync(binNode).isSymbolicLink()).toBe(true)
  expect(fs.existsSync(binNode)).toBe(false)

  await env.handler({
    bin: binDir,
    global: true,
    pnpmHomeDir,
    configByUri: {},
  }, ['rm', '18.12'])

  expect(fs.existsSync(binNode)).toBe(false)
  let linkExists = true
  try {
    fs.lstatSync(binNode)
  } catch {
    linkExists = false
  }
  expect(linkExists).toBe(false)

  fs.rmSync(tempDir, { recursive: true, force: true })
})

test('env remove cleans up Windows cmd shims and executables without a symlink', async () => {
  const originalPlatform = process.platform
  Object.defineProperty(process, 'platform', { value: 'win32' })
  try {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-env-test-'))
    const binDir = path.join(tempDir, 'bin')
    const pnpmHomeDir = path.join(tempDir, 'home')
    const nodejsDir = path.join(pnpmHomeDir, 'nodejs')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(path.join(nodejsDir, '18.12.0'), { recursive: true })

    const cmdShim = path.join(binDir, 'node.cmd')
    const ps1Shim = path.join(binDir, 'node.ps1')
    const exeShim = path.join(binDir, 'node.exe')
    fs.writeFileSync(cmdShim, '@"%~dp0\\..\\home\\nodejs\\18.12.0\\node.exe" %*')
    fs.writeFileSync(ps1Shim, '& "$PSScriptRoot\\..\\home\\nodejs\\18.12.0\\node.exe" @args')
    fs.writeFileSync(exeShim, 'fake-binary')

    await env.handler({
      bin: binDir,
      global: true,
      pnpmHomeDir,
      configByUri: {},
    }, ['remove', '18.12.0'])

    expect(fs.existsSync(cmdShim)).toBe(false)
    expect(fs.existsSync(ps1Shim)).toBe(false)
    expect(fs.existsSync(exeShim)).toBe(false)

    fs.rmSync(tempDir, { recursive: true, force: true })
  } finally {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  }
})


