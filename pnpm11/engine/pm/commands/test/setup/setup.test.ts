import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import type { PathExtenderReport } from '@pnpm/os.env.path-extender'

jest.unstable_mockModule('@pnpm/os.env.path-extender', () => ({
  addDirToEnvPath: jest.fn(),
}))

const actualCliMeta = await import('@pnpm/cli.meta')
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  ...actualCliMeta,
  detectIfCurrentPkgIsExecutable: jest.fn(() => false),
}))

const actualChildProcess = await import('node:child_process')
jest.unstable_mockModule('node:child_process', () => ({
  ...actualChildProcess,
  spawnSync: jest.fn(() => ({ status: 0 })),
}))

const actualFs = await import('node:fs')
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

const { addDirToEnvPath } = await import('@pnpm/os.env.path-extender')
const { detectIfCurrentPkgIsExecutable } = await import('@pnpm/cli.meta')
const { spawnSync } = await import('node:child_process')
const { setup } = await import('@pnpm/engine.pm.commands')

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

test('global install of the standalone executable skips its build scripts', async () => {
  jest.mocked(addDirToEnvPath).mockReturnValue(Promise.resolve<PathExtenderReport>({
    oldSettings: 'PNPM_HOME=dir',
    newSettings: 'PNPM_HOME=dir',
  }))
  jest.mocked(detectIfCurrentPkgIsExecutable).mockReturnValue(true)
  const tmpDir = actualFs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-setup-test-'))
  const execPath = path.join(tmpDir, 'pnpm')
  const originalExecPath = process.execPath
  Object.defineProperty(process, 'execPath', { value: execPath, configurable: true })
  try {
    await setup.handler({ pnpmHomeDir: path.join(tmpDir, 'home') })
  } finally {
    Object.defineProperty(process, 'execPath', { value: originalExecPath, configurable: true })
    actualFs.rmSync(tmpDir, { recursive: true, force: true })
  }
  expect(spawnSync).toHaveBeenCalledTimes(1)
  const args = jest.mocked(spawnSync).mock.calls[0][1] as string[]
  expect(args).toContain('--ignore-scripts')
  expect(args).toEqual(['add', '-g', '--ignore-scripts', `file:${tmpDir}`])
})
