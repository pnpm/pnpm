import { beforeEach, expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'

const mockRunPnpmCli = jest.fn()
jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: mockRunPnpmCli,
}))

const { runtime } = await import('@pnpm/engine.runtime.commands')

beforeEach(() => {
  mockRunPnpmCli.mockClear()
})

test('runtime set calls pnpm add with the correct arguments globally', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    cacheDir: '/tmp/cache',
    dir: '/tmp/project',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    storeDir: '/tmp/store',
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22', '--save-dev', '--global', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store', '--cache-dir', '/tmp/cache'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('runtime set defaults to --save-dev so the runtime lands in devEngines', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: false,
    pnpmHomeDir: '/tmp/pnpm-home',
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22', '--save-dev', '--ignore-workspace-root-check'],
    { cwd: '/tmp/project' }
  )
})

test('runtime set with --save-prod saves the runtime under engines', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: false,
    pnpmHomeDir: '/tmp/pnpm-home',
    saveProd: true,
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22', '--save-prod', '--ignore-workspace-root-check'],
    { cwd: '/tmp/project' }
  )
})

test('runtime set with --save-dev keeps the runtime under devEngines (matches the default)', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: false,
    pnpmHomeDir: '/tmp/pnpm-home',
    saveDev: true,
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22', '--save-dev', '--ignore-workspace-root-check'],
    { cwd: '/tmp/project' }
  )
})

test('runtime set with both --save-dev and --save-prod prefers --save-dev (matches getSaveType precedence)', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: false,
    pnpmHomeDir: '/tmp/pnpm-home',
    saveDev: true,
    saveProd: true,
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22', '--save-dev', '--ignore-workspace-root-check'],
    { cwd: '/tmp/project' }
  )
})

test('runtime set without version spec', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
  }, ['set', 'node'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:', '--save-dev', '--global', '--global-bin-dir', '/usr/local/bin'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('runtime set works with deno', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
  }, ['set', 'deno', '2'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'deno@runtime:2', '--save-dev', '--global', '--global-bin-dir', '/usr/local/bin'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('fail if no subcommand is given', async () => {
  await expect(
    runtime.handler({
      bin: '/usr/local/bin',
      dir: '/tmp/project',
      global: true,
      pnpmHomeDir: '/tmp/pnpm-home',
    }, [])
  ).rejects.toEqual(new PnpmError('RUNTIME_NO_SUBCOMMAND', 'Please specify the subcommand'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('fail if unknown subcommand is given', async () => {
  await expect(
    runtime.handler({
      bin: '/usr/local/bin',
      dir: '/tmp/project',
      global: true,
      pnpmHomeDir: '/tmp/pnpm-home',
    }, ['foo'])
  ).rejects.toEqual(new PnpmError('RUNTIME_UNKNOWN_SUBCOMMAND', 'Unknown subcommand: foo'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})

test('fail if runtime name is missing', async () => {
  await expect(
    runtime.handler({
      bin: '/usr/local/bin',
      dir: '/tmp/project',
      global: true,
      pnpmHomeDir: '/tmp/pnpm-home',
    }, ['set'])
  ).rejects.toEqual(new PnpmError('MISSING_RUNTIME_NAME', '"pnpm runtime set <name> <version>" requires a runtime name (e.g. node, deno, bun)'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})
