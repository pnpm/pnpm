import { inheritAuthConfig, inheritDlxConfig } from './auth.js'
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
})

test('inheritDlxConfig copies auth and security policy keys from source to target', () => {
  const target: InheritableConfigPair = {
    config: {
      bin: 'foo',
      cacheDir: '/path/to/cache/dir',
      registry: 'https://npmjs.com/registry/',
      shamefullyHoist: true,
      authConfig: {
        registry: 'https://npmjs.com/registry/',
      },
    } as any, // eslint-disable-line
  }

  inheritDlxConfig(target, {
    config: {
      bin: 'bar',
      cacheDir: '/path/to/another/cache/dir',
      storeDir: '/path/to/custom/store/dir',
      registry: 'https://example.com/local-registry/',
      shamefullyHoist: false,
      minimumReleaseAge: 1440,
      minimumReleaseAgeExclude: ['trusted-pkg'],
      minimumReleaseAgeStrict: true,
      trustPolicy: 'no-downgrade',
      trustPolicyExclude: ['legacy-pkg'],
      trustPolicyIgnoreAfter: 525600,
      authConfig: {
        registry: 'https://example.com/local-registry/',
        '//example.com/local-registry/:_authToken': 'SECRET_TOKEN',
        'minimum-release-age': '1440',
      },
    } as any, // eslint-disable-line
  })

  // Auth keys are inherited
  expect(target.config.registry).toBe('https://example.com/local-registry/')

  // Security/trust policy keys are inherited
  expect(target.config.minimumReleaseAge).toBe(1440)
  expect(target.config.minimumReleaseAgeExclude).toEqual(['trusted-pkg'])
  expect(target.config.minimumReleaseAgeStrict).toBe(true)
  expect(target.config.trustPolicy).toBe('no-downgrade')
  expect(target.config.trustPolicyExclude).toEqual(['legacy-pkg'])
  expect(target.config.trustPolicyIgnoreAfter).toBe(525600)

  // Project-structural keys are NOT inherited
  expect(target.config.bin).toBe('foo')
  expect(target.config.cacheDir).toBe('/path/to/cache/dir')
  expect(target.config.shamefullyHoist).toBe(true)
  expect(target.config.storeDir).toBeUndefined()

  // Raw auth keys are inherited, raw policy keys are inherited
  expect(target.config.authConfig).toMatchObject({
    registry: 'https://example.com/local-registry/',
    '//example.com/local-registry/:_authToken': 'SECRET_TOKEN',
    'minimum-release-age': '1440',
  })
})
