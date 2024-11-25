import path from 'path'
import fs from 'fs'
import { globalWarn } from '@pnpm/logger'
import { type VerifyDepsBeforeRun } from '@pnpm/config'
import { run } from '@pnpm/plugin-commands-script-runners'
import { prepare } from '@pnpm/prepare'
import { prompt } from 'enquirer'
import { DEFAULT_OPTS } from './utils'

jest.mock('@pnpm/logger', () => {
  const originalModule = jest.requireActual('@pnpm/logger')
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

jest.mock('enquirer', () => ({
  prompt: jest.fn(),
}))

delete process.env['pnpm_run_skip_deps_check']

const rootProjectManifest = {
  name: 'root',
  private: true,
  dependencies: {
    '@pnpm.e2e/foo': '100.0.0',
  },
  scripts: {
    test: 'echo hello from script',
  },
}

async function runTest (verifyDepsBeforeRun: VerifyDepsBeforeRun): Promise<void> {
  await run.handler({
    ...DEFAULT_OPTS,
    bin: 'node_modules/.bin',
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    pnpmHomeDir: '',
    rawConfig: {},
    verifyDepsBeforeRun,
    rootProjectManifest,
    rootProjectManifestDir: process.cwd(),
  }, ['test'])
}

test('throw an error if verifyDepsBeforeRun is set to error', async () => {
  prepare(rootProjectManifest)

  let err!: Error
  try {
    await runTest('error')
  } catch (_err) {
    err = _err as Error
  }
  expect(err.message).toContain('Cannot find a lockfile in')
})

test('install the dependencies if verifyDepsBeforeRun is set to install', async () => {
  prepare(rootProjectManifest)

  await runTest('install')

  expect(fs.existsSync(path.resolve('node_modules'))).toBeTruthy()
})

test('log a warning if verifyDepsBeforeRun is set to warn', async () => {
  prepare(rootProjectManifest)

  await runTest('warn')

  expect(globalWarn).toHaveBeenCalledWith(
    expect.stringContaining('Your node_modules are out of sync with your lockfile')
  )
  expect(fs.existsSync(path.resolve('node_modules'))).toBeFalsy()
})

test('prompt the user if verifyDepsBeforeRun is set to prompt', async () => {
  prepare(rootProjectManifest)

  // Mock the user confirming the prompt
  ;(prompt as jest.Mock).mockResolvedValue({ runInstall: true })

  await runTest('prompt')

  expect(prompt).toHaveBeenCalledWith({
    type: 'confirm',
    name: 'runInstall',
    message: expect.stringContaining(
      'Your "node_modules" directory is out of sync with the "pnpm-lock.yaml" file'
    ),
    initial: true,
  })

  expect(fs.existsSync(path.resolve('node_modules'))).toBeTruthy()
})
