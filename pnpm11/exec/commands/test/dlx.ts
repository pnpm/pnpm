import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('execa', () => ({
  safeExeca: jest.fn(),
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
