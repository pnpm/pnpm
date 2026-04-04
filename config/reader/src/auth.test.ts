import { inheritAuthConfig } from './auth.js'
import type { InheritableConfigPair } from './inheritPickedConfig.js'

test('inheritAuthConfig copies only auth keys from source to target', () => {
  const target: InheritableConfigPair = {
    config: {
      bin: 'foo',
      cacheDir: '/path/to/cache/dir',
      registry: 'https://npmjs.com/registry/',
      authConfig: {
        'cache-dir': '/path/to/cache/dir',
        registry: 'https://npmjs.com/registry/',
      },
    } as any, // eslint-disable-line
    context: {
      rawLocalConfig: {
        bin: 'foo',
        registry: 'https://npmjs.com/registry/',
      },
    },
  }

  inheritAuthConfig(target, {
    config: {
      bin: 'bar',
      cacheDir: '/path/to/another/cache/dir',
      storeDir: '/path/to/custom/store/dir',
      registry: 'https://example.com/local-registry/',
      authConfig: {
        registry: 'https://example.com/global-registry/',
        '//example.com/global-registry/:_auth': 'MY_SECRET_GLOBAL_AUTH',
      },
    } as any, // eslint-disable-line
    context: {
      rawLocalConfig: {
        bin: 'bar',
        'cache-dir': '/path/to/another/cache/dir',
        'store-dir': '/path/to/custom/store/dir',
        registry: 'https://example.com/local-registry/',
        '//example.com/local-registry/:_authToken': 'MY_SECRET_LOCAL_AUTH',
      },
    },
  })

  expect(target.config).toMatchObject({
    bin: 'foo',
    cacheDir: '/path/to/cache/dir',
    registry: 'https://example.com/local-registry/',
    authConfig: {
      'cache-dir': '/path/to/cache/dir',
      registry: 'https://example.com/global-registry/',
      '//example.com/global-registry/:_auth': 'MY_SECRET_GLOBAL_AUTH',
    },
  })
  expect(target.context.rawLocalConfig).toStrictEqual({
    bin: 'foo',
    registry: 'https://example.com/local-registry/',
    '//example.com/local-registry/:_authToken': 'MY_SECRET_LOCAL_AUTH',
  })
})
