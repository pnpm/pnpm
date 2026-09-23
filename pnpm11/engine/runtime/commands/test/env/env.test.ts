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

test('env remove calls pnpm remove with the correct arguments', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    cacheDir: '/tmp/cache',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    configByUri: {},
    storeDir: '/tmp/store',
  }, ['remove', '18'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['remove', '--global', 'node', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store', '--cache-dir', '/tmp/cache'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('env rm alias works identically to env remove', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    configByUri: {},
  }, ['rm', '20'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['remove', '--global', 'node', '--global-bin-dir', '/usr/local/bin'],
    { cwd: '/tmp/pnpm-home' }
  )
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
  mockRunPnpmCli.mockImplementation(() => {
    throw new Error('not in global packages')
  })

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


