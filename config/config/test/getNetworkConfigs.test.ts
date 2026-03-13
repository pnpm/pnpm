import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { type NetworkConfigs, getNetworkConfigs } from '../src/getNetworkConfigs.js'

test('without files', () => {
  expect(getNetworkConfigs({})).toStrictEqual({
    registries: {},
    sslConfigs: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    sslConfigs: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '//example.com/foo:ca': 'some-ca',
  })).toStrictEqual({
    registries: {},
    sslConfigs: {
      '//example.com/foo': {
        ca: 'some-ca',
        cert: '',
        key: '',
      },
    },
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '//example.com/foo:cert': 'some-cert',
  })).toStrictEqual({
    registries: {},
    sslConfigs: {
      '//example.com/foo': {
        cert: 'some-cert',
        key: '',
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
    sslConfigs: {
      '//example.com/foo': {
        ca: 'some-ca',
        cert: 'some-cert',
        key: '',
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
    sslConfigs: {
      '//example.com/foo': {
        ca: 'some-ca',
        cert: 'some-cert',
        key: '',
      },
    },
  } as NetworkConfigs)
})

test('auth infos', () => {
  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:_authToken': 'example auth token',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    authInfos: {
      '//example.com/foo': {
        authToken: 'example auth token',
      },
    },
    sslConfigs: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:_auth': btoa('foo:bar'),
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    authInfos: {
      '//example.com/foo': {
        authUserPass: {
          username: 'foo',
          password: 'bar',
        },
      },
    },
    sslConfigs: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:username': 'foo',
    '//example.com/foo:_password': btoa('bar'),
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    authInfos: {
      '//example.com/foo': {
        authUserPass: {
          username: 'foo',
          password: 'bar',
        },
      },
    },
    sslConfigs: {},
  } as NetworkConfigs)

  expect(getNetworkConfigs({
    '@foo:registry': 'https://example.com/foo',
    '//example.com/foo:tokenHelper': 'node ./my-token-helper.cjs',
  })).toStrictEqual({
    registries: {
      '@foo': 'https://example.com/foo',
    },
    authInfos: {
      '//example.com/foo': {
        tokenHelper: ['node', './my-token-helper.cjs'],
      },
    },
    sslConfigs: {},
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
    sslConfigs: {},
  } as NetworkConfigs)
})
