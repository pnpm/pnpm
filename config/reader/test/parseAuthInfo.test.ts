import {
  type Creds,
  AuthMissingSeparatorError,
  parseAuthInfo,
  TokenHelperUnsupportedCharacterError,
} from '../src/parseAuthInfo.js'

describe('parseAuthInfo', () => {
  test('empty object', () => {
    expect(parseAuthInfo({})).toBeUndefined()
  })

  test('authToken', () => {
    expect(parseAuthInfo({
      authToken: 'example auth token',
    })).toStrictEqual({
      authToken: 'example auth token',
    } as Creds)
  })

  test('authPairBase64', () => {
    expect(parseAuthInfo({
      authPairBase64: btoa('foo:bar'),
    })).toStrictEqual({
      authUserPass: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)

    expect(parseAuthInfo({
      authPairBase64: btoa('foo:bar:baz'),
    })).toStrictEqual({
      authUserPass: {
        username: 'foo',
        password: 'bar:baz',
      },
    } as Creds)
  })

  test('authPairBase64 must have a separator', () => {
    expect(() => parseAuthInfo({
      authPairBase64: btoa('foo'),
    })).toThrow(new AuthMissingSeparatorError())
  })

  test('authUsername and authPassword', () => {
    expect(parseAuthInfo({
      authUsername: 'foo',
      authPassword: btoa('bar'),
    })).toStrictEqual({
      authUserPass: {
        username: 'foo',
        password: 'bar',
      },
    } as Creds)

    expect(parseAuthInfo({
      authUsername: 'foo',
    })).toBeUndefined()

    expect(parseAuthInfo({
      authPassword: 'bar',
    })).toBeUndefined()
  })

  test('tokenHelper', () => {
    expect(parseAuthInfo({
      tokenHelper: 'example-token-helper --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['example-token-helper', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseAuthInfo({
      tokenHelper: './example-token-helper.sh --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['./example-token-helper.sh', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseAuthInfo({
      tokenHelper: 'node ./example-token-helper.js --foo --bar baz',
    })).toStrictEqual({
      tokenHelper: ['node', './example-token-helper.js', '--foo', '--bar', 'baz'],
    } as Creds)

    expect(parseAuthInfo({
      tokenHelper: './example-token-helper.sh',
    })).toStrictEqual({
      tokenHelper: ['./example-token-helper.sh'],
    } as Creds)
  })

  test('tokenHelper does not support environment variable', () => {
    expect(() => parseAuthInfo({
      tokenHelper: 'example-token-helper $MY_VAR',
    })).toThrow(new TokenHelperUnsupportedCharacterError('$'))
  })

  test('tokenHelper does not support quotations', () => {
    expect(() => parseAuthInfo({
      tokenHelper: 'example-token-helper "hello world"',
    })).toThrow(new TokenHelperUnsupportedCharacterError('"'))

    expect(() => parseAuthInfo({
      tokenHelper: "example-token-helper 'hello world'",
    })).toThrow(new TokenHelperUnsupportedCharacterError("'"))
  })
})
