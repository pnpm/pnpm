import { expect, test } from '@jest/globals'

import type { InheritableConfigPair } from './inheritPickedConfig.js'
import { inheritAuthConfig, inheritDlxConfig } from './localConfig.js'

test('inheritAuthConfig copies only auth keys from source to target', () => {
  const target: InheritableConfigPair = {
    config: {
      bin: 'foo',
      cacheDir: '/path/to/cache/dir',
      registry: 'https://npmjs.com/registry/',
      authConfig: {
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
    },
  })

  expect(target.config).toMatchObject({
    bin: 'foo',
    cacheDir: '/path/to/cache/dir',
    registry: 'https://example.com/local-registry/',
    authConfig: {
      registry: 'https://example.com/global-registry/',
      '//example.com/global-registry/:_auth': 'MY_SECRET_GLOBAL_AUTH',
    },
  })
})

test('inheritDlxConfig copies auth, security policy, and nodeDownloadMirrors from source to target', () => {
  const target: InheritableConfigPair = {
    config: {
      bin: 'foo',
      cacheDir: '/path/to/cache/dir',
      registry: 'https://npmjs.com/registry/',
      shamefullyHoist: true,
      authConfig: {
        registry: 'https://npmjs.com/registry/',
      },
    },
  }

  inheritDlxConfig(target, {
    config: {
      bin: 'bar',
      cacheDir: '/path/to/another/cache/dir',
      storeDir: '/path/to/custom/store/dir',
      registry: 'https://example.com/local-registry/',
      shamefullyHoist: false,
      nodeDownloadMirrors: { release: 'https://mirror.example/nodejs/' },
      minimumReleaseAge: 1440,
      minimumReleaseAgeExclude: ['trusted-pkg'],
      minimumReleaseAgeStrict: true,
      trustPolicy: 'no-downgrade',
      trustPolicyExclude: ['legacy-pkg'],
      trustPolicyIgnoreAfter: 525600,
      authConfig: {
        registry: 'https://example.com/local-registry/',
        '//example.com/local-registry/:_authToken': 'SECRET_TOKEN',
      },
    },
  })

  // Auth, trust-policy, and Node mirror keys are inherited from the local workspace;
  // project-structural keys (bin, cacheDir, shamefullyHoist) keep their target values.
  expect(target.config).toMatchObject({
    bin: 'foo',
    cacheDir: '/path/to/cache/dir',
    shamefullyHoist: true,
    registry: 'https://example.com/local-registry/',
    nodeDownloadMirrors: { release: 'https://mirror.example/nodejs/' },
    minimumReleaseAge: 1440,
    minimumReleaseAgeExclude: ['trusted-pkg'],
    minimumReleaseAgeStrict: true,
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['legacy-pkg'],
    trustPolicyIgnoreAfter: 525600,
    authConfig: {
      registry: 'https://example.com/local-registry/',
      '//example.com/local-registry/:_authToken': 'SECRET_TOKEN',
    },
  })
  // storeDir exists only on the source, must not be inherited.
  expect(target.config.storeDir).toBeUndefined()
})
