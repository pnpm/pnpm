import { describe, expect, test } from '@jest/globals'

import {
  AuthBase64DecodeError,
  AuthMissingSeparatorError,
  type Creds,
  parseCreds,
  TokenHelperUnsupportedCharacterError,
} from '../src/parseCreds.js'

describe('parseCreds', () => {
  test('empty object', () => {
    expect(parseCreds({})).toBeUndefined()
  })

  test('authToken', () => {
    expect(parseCreds({
      authToken: 'example auth token',
    })).toStrictEqual({
      authToken: 'example auth token',
    } as Creds)
  })

  test('empty authToken is treated as absent', () => {
    // Reachable path via loadNpmrcFiles when an unresolved `${VAR}` placeholder
    // is dropped to empty. Must not surface as a usable token.
    expect(parseCreds({ authToken: '' })).toBeUndefined()
  })

  test('authPairBase64', () => {
    expect(parseCreds({
      authPairBase64: btoa('foo:bar'),
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)

    expect(parseCreds({
      authPairBase64: btoa('foo:bar:baz'),
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar:baz',
      },
    } as Creds)
  })

  test('authPairBase64 must have a separator', () => {
    expect(() => parseCreds({
      authPairBase64: btoa('foo'),
    })).toThrow(new AuthMissingSeparatorError())
  })

  test('authPairBase64 allows redundant trailing padding', () => {
    expect(parseCreds({
      authPairBase64: `${btoa('foo:bar')}=`,
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)
  })

  test('authPairBase64 allows missing trailing padding', () => {
    expect(parseCreds({
      authPairBase64: btoa('foo:bar').replaceAll('=', ''),
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)
  })

  test('authPairBase64 of only padding is not base64', () => {
    expect(() => parseCreds({
      authPairBase64: '====',
    })).toThrow(new AuthBase64DecodeError('_auth'))
  })

  test('an empty authPairBase64 supplies no credentials', () => {
    expect(parseCreds({ authPairBase64: '' })).toBeUndefined()
  })

  test('authPairBase64 must be base64', () => {
    expect(() => parseCreds({
      authPairBase64: 'foo*bar',
    })).toThrow(new AuthBase64DecodeError('_auth'))
  })

  test('authPairBase64 throws a descriptive error when value is an unresolved env placeholder', () => {
    try {
      parseCreds({
        authPairBase64: '${ENV_VAR_123}',
      })
      throw new Error('expected parseCreds to throw')
    } catch (err: any) { // eslint-disable-line
      expect(err).toMatchObject({
        code: 'ERR_PNPM_AUTH_INVALID_BASE64',
        message: 'Failed to decode _auth as base64',
        hint: 'The _auth value still contains an unresolved env placeholder. Make sure the referenced env var is set in the config source that defines this credential before running pnpm.',
      })
    }
  })

  test('authUsername and authPassword', () => {
    expect(parseCreds({
      authUsername: 'foo',
      authPassword: btoa('bar'),
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)

    expect(parseCreds({
      authUsername: 'foo',
    })).toBeUndefined()

    expect(parseCreds({
      authPassword: 'bar',
    })).toBeUndefined()
  })

  test('authPassword allows redundant trailing padding', () => {
    expect(parseCreds({
      authUsername: 'foo',
      authPassword: `${btoa('bar')}=`,
    })).toStrictEqual({
      basicAuth: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)
  })

  test('authPassword must be base64', () => {
    expect(() => parseCreds({
      authUsername: 'foo',
      authPassword: 'bar*baz',
    })).toThrow(new AuthBase64DecodeError('_password'))
  })

  test('authPassword throws a descriptive error when value is an unresolved env placeholder', () => {
    try {
      parseCreds({
        authUsername: 'foo',
        authPassword: '${ENV_VAR_123}',
      })
      throw new Error('expected parseCreds to throw')
    } catch (err: any) { // eslint-disable-line
      expect(err).toMatchObject({
        code: 'ERR_PNPM_AUTH_INVALID_BASE64',
        message: 'Failed to decode _password as base64',
        hint: 'The _password value still contains an unresolved env placeholder. Make sure the referenced env var is set in the config source that defines this credential before running pnpm.',
      })
    }
  })

  test('tokenHelper', () => {
    expect(parseCreds({
      tokenHelper: 'example-token-helper --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['example-token-helper', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseCreds({
      tokenHelper: './example-token-helper.sh --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['./example-token-helper.sh', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseCreds({
      tokenHelper: 'node ./example-token-helper.js --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['node', './example-token-helper.js', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseCreds({
      tokenHelper: './example-token-helper.sh',
    })).toStrictEqual({
      tokenHelper: ['./example-token-helper.sh'],
    } as Creds)
  })

  test('tokenHelper does not support environment variable', () => {
    expect(() => parseCreds({
      tokenHelper: 'example-token-helper $MY_VAR',
    })).toThrow(new TokenHelperUnsupportedCharacterError('$'))
  })

  test('tokenHelper does not support quotations', () => {
    expect(() => parseCreds({
      tokenHelper: 'example-token-helper "hello world"',
    })).toThrow(new TokenHelperUnsupportedCharacterError('"'))

    expect(() => parseCreds({
      tokenHelper: "example-token-helper 'hello world'",
    })).toThrow(new TokenHelperUnsupportedCharacterError("'"))
  })
})
