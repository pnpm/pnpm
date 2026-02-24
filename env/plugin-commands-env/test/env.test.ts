import { jest } from '@jest/globals'
import { PnpmError } from '@pnpm/error'

const mockRunPnpmCli = jest.fn()
jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: mockRunPnpmCli,
}))

const { env } = await import('@pnpm/plugin-commands-env')

beforeEach(() => {
  mockRunPnpmCli.mockClear()
})

test('env use calls pnpm add with the correct arguments', async () => {
  await env.handler({
    bin: '/usr/local/bin',
    cacheDir: '/tmp/cache',
    global: true,
    pnpmHomeDir: '/tmp/pnpm-home',
    rawConfig: {},
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
    rawConfig: {},
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
    rawConfig: {},
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
      rawConfig: {},
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
      rawConfig: {},
    }, ['use', 'lts'])
  ).rejects.toEqual(new PnpmError('CANNOT_MANAGE_NODE', 'Unable to manage Node.js because pnpm was not installed using the standalone installation script'))

  expect(mockRunPnpmCli).not.toHaveBeenCalled()
})
