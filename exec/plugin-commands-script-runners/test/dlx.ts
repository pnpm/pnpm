import path from 'path'
import execa from 'execa'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils'

jest.mock('execa')

beforeEach((execa as jest.Mock).mockClear)

test('dlx should work with scoped packages', async () => {
  prepareEmpty()
  const userAgent = 'pnpm/0.0.0'

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    userAgent,
  }, ['@foo/touch-file-one-bin'])

  expect(execa).toBeCalledWith('touch-file-one-bin', [], expect.objectContaining({
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

  expect(execa).toBeCalledWith('touch-file-one-bin', [], expect.anything())
})
