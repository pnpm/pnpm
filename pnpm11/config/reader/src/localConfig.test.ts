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
  expect(target.config.storeDir).toBeUndefined()
})

test('inheritDlxConfig inherits only the release nodeDownloadMirrors entry', () => {
  const target: InheritableConfigPair = {
    config: {
      nodeDownloadMirrors: {
        release: 'https://global.example/release/',
        nightly: 'https://global.example/nightly/',
      },
      authConfig: {},
    },
  }

  inheritDlxConfig(target, {
    config: {
      nodeDownloadMirrors: {
        release: 'https://workspace.example/release/',
        nightly: 'https://workspace.example/nightly/',
        rc: 'https://workspace.example/rc/',
      },
      authConfig: {},
    },
  })

  // Only `release` ships a signed SHASUMS256.txt, so it is the only channel a
  // workspace may redirect. A workspace `nightly`/`rc` mirror would supply both
  // the archive and its checksum for a runtime that dlx executes.
  expect(target.config.nodeDownloadMirrors).toStrictEqual({
    release: 'https://workspace.example/release/',
    nightly: 'https://global.example/nightly/',
  })
})

test('inheritDlxConfig leaves nodeDownloadMirrors alone when the workspace sets no release mirror', () => {
  const target: InheritableConfigPair = {
    config: {
      nodeDownloadMirrors: { nightly: 'https://global.example/nightly/' },
      authConfig: {},
    },
  }

  inheritDlxConfig(target, {
    config: {
      nodeDownloadMirrors: { nightly: 'https://workspace.example/nightly/' },
      authConfig: {},
    },
  })

  expect(target.config.nodeDownloadMirrors).toStrictEqual({
    nightly: 'https://global.example/nightly/',
  })
})
