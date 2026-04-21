import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { getNetworkConfigs, type NetworkConfigs } from '../src/getNetworkConfigs.js'

test('without files', () => {
  expect(getNetworkConfigs({})).toStrictEqual({
    registries: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '//example.com/foo:ca': 'some-ca',
  })).toStrictEqual({
    registries: {},
    configByUri: {
      '//example.com/foo': {
        tls: { ca: 'some-ca' },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '//example.com/foo:cert': 'some-cert',
  })).toStrictEqual({
    registries: {},
    configByUri: {
      '//example.com/foo': {
        tls: { cert: 'some-cert' },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:ca': 'some-ca',
    '//example.com/foo:cert': 'some-cert',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        tls: { ca: 'some-ca', cert: 'some-cert' },
      },
    },
  } as NetworkConfigs)
})

test('with files', () => {
  prepareEmpty()

  fs.writeFileSync('cafile', 'some-ca')
  fs.writeFileSync('certfile', 'some-cert')

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:cafile': 'cafile',
    '//example.com/foo:certfile': 'certfile',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        tls: { ca: 'some-ca', cert: 'some-cert' },
      },
    },
  } as NetworkConfigs)
})

test('auth and tls combined', () => {
  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:_authToken': 'example auth token',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        creds: { authToken: 'example auth token' },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:_auth': btoa('foo:bar'),
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        creds: {
          basicAuth: {
            username: 'foo',
            password: 'bar',
          },
        },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:username': 'foo',
    '//example.com/foo:_password': btoa('bar'),
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        creds: {
          basicAuth: {
            username: 'foo',
            password: 'bar',
          },
        },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:tokenHelper': 'node ./my-token-helper.cjs',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    configByUri: {
      '//example.com/foo': {
        creds: { tokenHelper: ['node', './my-token-helper.cjs'] },
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '//example.com/foo:_authToken': 'token',
    '//example.com/foo:cert': 'some-cert',
    '//example.com/foo:key': 'some-key',
  })).toStrictEqual({
    registries: {},
    configByUri: {
      '//example.com/foo': {
        creds: { authToken: 'token' },
        tls: { cert: 'some-cert', key: 'some-key' },
      },
    },
  } as NetworkConfigs)
})

test('unsupported key', () => {
  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:someUnsupportedKey': 'hello world',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
  } as NetworkConfigs)
})
