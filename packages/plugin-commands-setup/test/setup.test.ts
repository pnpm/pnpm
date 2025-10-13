import { PnpmError } from '@pnpm/error'
import { jest } from '@jest/globals'
import { type PathExtenderReport } from '@pnpm/os.env.path-extender'

jest.unstable_mockModule('@pnpm/os.env.path-extender', () => ({
  addDirToEnvPath: jest.fn(),
}))

const actualFs = await import('fs')
jest.unstable_mockModule('fs', () => {
  return {
    ...actualFs,
    promises: {
      ...actualFs.promises,
      readFile: jest.fn(),
      writeFile: jest.fn(),
    },
  }
})

jest.mock('@zkochan/cmd-shim', () => ({
  __esModule: true,
  default: jest.fn(),
}))

const { addDirToEnvPath } = await import('@pnpm/os.env.path-extender')
const { setup } = await import('@pnpm/plugin-commands-setup')

test('setup makes no changes', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe('No changes to the environment were made. Everything is already up to date.')
})

test('setup makes changes on POSIX', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    configFile: {
      changeType: 'created',
      path: '~/.bashrc',
    },
    oldSettings: 'export PNPM_HOME=dir1',
    newSettings: 'export PNPM_HOME=dir2',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe(`Created ~/.bashrc

Next configuration changes were made:
export PNPM_HOME=dir2

To start using pnpm, run:
source ~/.bashrc
`)
})

test('setup makes changes on Windows', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'export PNPM_HOME=dir1',
    newSettings: 'export PNPM_HOME=dir2',
  }))
  const output = await setup.handler({ pnpmHomeDir: '' })
  expect(output).toBe(`Next configuration changes were made:
export PNPM_HOME=dir2

Setup complete. Open a new terminal to start using pnpm.`)
})

test('hint is added to ERR_PNPM_BAD_ENV_FOUND error object', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.reject(new PnpmError('BAD_ENV_FOUND', '')))
  let err!: PnpmError
  try {
    await setup.handler({ pnpmHomeDir: '' })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.hint).toBe('If you want to override the existing env variable, use the --force option')
})

test('hint is added to ERR_PNPM_BAD_SHELL_SECTION error object', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.reject(new PnpmError('BAD_SHELL_SECTION', '')))
  let err!: PnpmError
  try {
    await setup.handler({ pnpmHomeDir: '' })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err?.hint).toBe('If you want to override the existing configuration section, use the --force option')
})
