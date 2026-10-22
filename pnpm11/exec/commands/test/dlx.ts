import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'

import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('execa', () => ({
  // The handler registers the returned subprocess with the child process
  // tracker, so the mock returns a minimal subprocess-like object.
  safeExeca: jest.fn(() => ({ pid: undefined, once: jest.fn() })),
  sync: jest.fn(),
}))

const { safeExeca: execa } = await import('execa')
const { dlx } = await import('@pnpm/exec.commands')

beforeEach(() => {
  jest.mocked(execa).mockClear()
})

test('dlx should work with scoped packages', async () => {
  prepareEmpty()
  const userAgent = 'pnpm/0.0.0'

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    userAgent,
  }, ['@foo/touch-file-one-bin'])

  expect(execa).toHaveBeenCalledWith('touch-file-one-bin', [], expect.objectContaining({
    env: expect.objectContaining({
      npm_config_user_agent: userAgent,
      // Jest, not pnpm, is the entry here, so scripts get the pnpm on PATH.
      npm_execpath: expect.stringMatching(/pnpm(?:\.\w+)?$/i),
      INIT_CWD: process.cwd(),
      npm_node_execpath: process.env.NODE || process.execPath,
      NODE: process.env.NODE || process.execPath,
    }),
  }))
})

test('dlx should work with versioned packages', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
  }, ['@foo/touch-file-one-bin@latest'])

  expect(execa).toHaveBeenCalledWith('touch-file-one-bin', [], expect.anything())
})

test('dlx without a command prints help', async () => {
  prepareEmpty()

  const result = await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
  }, [])

  expect(result.exitCode).toBe(1)
  expect(result.output).toContain('pnpm dlx <command> [args...]')
  expect(execa).not.toHaveBeenCalled()
})

test('dlx with --package but no command throws a missing command error', async () => {
  prepareEmpty()

  const err = await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    package: ['@foo/touch-file-one-bin'],
  }, []).catch((err: unknown) => err)

  expect(err).toBeInstanceOf(PnpmError)
  expect((err as PnpmError).code).toBe('ERR_PNPM_DLX_MISSING_COMMAND')
  expect((err as PnpmError).message).toBe("'pnpm dlx' requires a command to run")
  expect(execa).not.toHaveBeenCalled()
})
