import path from 'path'
import fs from 'fs'
import { type VerifyDepsBeforeRun } from '@pnpm/config'
import {
  run,
} from '@pnpm/plugin-commands-script-runners'
import { prepare } from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'

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
