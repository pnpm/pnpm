import { inheritAuthConfig } from './auth'
import { type InheritableConfig } from './inheritPickedConfig'

test('inheritAuthConfig copies only auth keys from source to target', () => {
  const target: InheritableConfig = {
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

  inheritAuthConfig(target, {
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

  expect(target).toStrictEqual({
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
