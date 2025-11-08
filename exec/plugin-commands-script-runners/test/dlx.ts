import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('execa', () => ({
  default: jest.fn(),
  sync: jest.fn(),
}))

const { default: execa } = await import('execa')
const { dlx } = await import('@pnpm/plugin-commands-script-runners')

beforeEach(() => jest.mocked(execa).mockClear())

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
