import { jest } from '@jest/globals'
import { PnpmError } from '@pnpm/error'

const mockRunPnpmCli = jest.fn()
jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: mockRunPnpmCli,
}))

const { runtime } = await import('@pnpm/runtime.commands')

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
    ['add', 'node@runtime:22', '--global', '--global-bin-dir', '/usr/local/bin', '--store-dir', '/tmp/store', '--cache-dir', '/tmp/cache'],
    { cwd: '/tmp/pnpm-home' }
  )
})

test('runtime set uses project dir when not global', async () => {
  await runtime.handler({
    bin: '/usr/local/bin',
    dir: '/tmp/project',
    global: false,
    pnpmHomeDir: '/tmp/pnpm-home',
  }, ['set', 'node', '22'])

  expect(mockRunPnpmCli).toHaveBeenCalledWith(
    ['add', 'node@runtime:22'],
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
    ['add', 'node@runtime:', '--global', '--global-bin-dir', '/usr/local/bin'],
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
    ['add', 'deno@runtime:2', '--global', '--global-bin-dir', '/usr/local/bin'],
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
