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

test('dlx inherits certain keys from local config', () => {
  const config: dlx.InheritConfig = {
    bin: 'foo',
    cacheDir: '/path/to/cache/dir',
    registry: 'https://npmjs.com/registry/',
    rawConfig: {
      'cache-dir': '/path/to/cache/dir',
      registry: 'https://npmjs.com/registry/',
    },
    rawLocalConfig: {
      bin: 'foo',
      registry: 'https://npmjs.com/registry/',
    },
  }

  dlx.inheritLocalConfig(config, {
    bin: 'bar',
    cacheDir: '/path/to/another/cache/dir',
    storeDir: '/path/to/custom/store/dir',
    registry: 'https://example.com/local-registry/',
    rawConfig: {
      registry: 'https://example.com/global-registry/',
      '//example.com/global-registry/:_auth': 'MY_SECRET_GLOBAL_AUTH',
    },
    rawLocalConfig: {
      bin: 'bar',
      'cache-dir': '/path/to/another/cache/dir',
      'store-dir': '/path/to/custom/store/dir',
      registry: 'https://example.com/local-registry/',
      '//example.com/local-registry/:_authToken': 'MY_SECRET_LOCAL_AUTH',
    },
  })

  expect(config).toStrictEqual({
    bin: 'foo',
    cacheDir: '/path/to/cache/dir',
    registry: 'https://example.com/local-registry/',
    rawConfig: {
      'cache-dir': '/path/to/cache/dir',
      registry: 'https://example.com/global-registry/',
      '//example.com/global-registry/:_auth': 'MY_SECRET_GLOBAL_AUTH',
    },
    rawLocalConfig: {
      bin: 'foo',
      registry: 'https://example.com/local-registry/',
      '//example.com/local-registry/:_authToken': 'MY_SECRET_LOCAL_AUTH',
    },
  })
})
