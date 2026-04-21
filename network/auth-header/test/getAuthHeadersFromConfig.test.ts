import os from 'node:os'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'

import { getAuthHeadersFromCreds } from '../src/getAuthHeadersFromConfig.js'

const osTokenHelper = {
  linux: path.join(import.meta.dirname, 'utils/test-exec.js'),
  win32: path.join(import.meta.dirname, 'utils/test-exec.bat'),
}

const osRawTokenHelper = {
  linux: path.join(import.meta.dirname, 'utils/test-exec-raw-token.js'),
  win32: path.join(import.meta.dirname, 'utils/test-exec-raw-token.bat'),
}

const osEmptyTokenHelper = {
  linux: path.join(import.meta.dirname, 'utils/test-exec-empty-token.js'),
  win32: path.join(import.meta.dirname, 'utils/test-exec-empty-token.bat'),
}

const osErrorTokenHelper = {
  linux: path.join(import.meta.dirname, 'utils/test-exec-error.js'),
  win32: path.join(import.meta.dirname, 'utils/test-exec-error.bat'),
}

// Only exception is win32, all others behave like linux
const osFamily = os.platform() === 'win32' ? 'win32' : 'linux'

describe('getAuthHeadersFromCreds()', () => {
  it('should convert auth token to Bearer header', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.npmjs.org/': { creds: { authToken: 'abc123'  } },
      '//registry.hu/': { creds: { authToken: 'def456'  } },
    }, '//registry.npmjs.org/')
    expect(result).toStrictEqual({
      '//registry.npmjs.org/': 'Bearer abc123',
      '//registry.hu/': 'Bearer def456',
    })
  })
  it('should convert basicAuth to Basic header', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { creds: { basicAuth: { username: 'foobar', password: 'foobar' }  } },
    }, '//registry.npmjs.org/')
    expect(result).toStrictEqual({
      '//registry.foobar.eu/': 'Basic Zm9vYmFyOmZvb2Jhcg==',
    })
  })
  it('should handle default registry auth (empty key)', () => {
    const result = getAuthHeadersFromCreds({
      '': { creds: { authToken: 'default-token'  } },
    }, '//reg.com/')
    expect(result).toStrictEqual({
      '//reg.com/': 'Bearer default-token',
    })
  })
  it('should execute tokenHelper', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { creds: { tokenHelper: [osTokenHelper[osFamily]]  } },
    }, '//registry.npmjs.org/')
    expect(result).toStrictEqual({
      '//registry.foobar.eu/': 'Bearer token-from-spawn',
    })
  })
  it('should prepend Bearer to raw token from tokenHelper', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { creds: { tokenHelper: [osRawTokenHelper[osFamily]]  } },
    }, '//registry.npmjs.org/')
    expect(result).toStrictEqual({
      '//registry.foobar.eu/': 'Bearer raw-token-no-scheme',
    })
  })
  it('should throw an error if the token helper fails', () => {
    expect(() => getAuthHeadersFromCreds({
      '//reg.com/': { creds: { tokenHelper: [osErrorTokenHelper[osFamily]]  } },
    }, '//registry.npmjs.org/')).toThrow('Exit code')
  })
  it('should throw an error if the token helper returns an empty token', () => {
    expect(() => getAuthHeadersFromCreds({
      '//reg.com/': { creds: { tokenHelper: [osEmptyTokenHelper[osFamily]]  } },
    }, '//registry.npmjs.org/')).toThrow('returned an empty token')
  })
  it('should return empty object when no auth infos', () => {
    const result = getAuthHeadersFromCreds({}, '//registry.npmjs.org/')
    expect(result).toStrictEqual({})
  })
})

