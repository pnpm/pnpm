import os from 'node:os'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'

import { getAuthHeadersByScope, getAuthHeadersFromCreds } from '../src/getAuthHeadersFromConfig.js'

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
      '//registry.npmjs.org/': { '@': { authToken: 'abc123' } },
      '//registry.hu/': { '@': { authToken: 'def456' } },
    })
    expect(result).toStrictEqual({
      authHeaderValueByURI: {
        '//registry.npmjs.org/': 'Bearer abc123',
        '//registry.hu/': 'Bearer def456',
      },
      scopedAuthHeaderValueByURI: {},
    })
  })
  it('should convert basicAuth to Basic header', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { '@': { basicAuth: { username: 'foobar', password: 'foobar' } } },
    })
    expect(result).toStrictEqual({
      authHeaderValueByURI: {
        '//registry.foobar.eu/': 'Basic Zm9vYmFyOmZvb2Jhcg==',
      },
      scopedAuthHeaderValueByURI: {},
    })
  })
  it('should execute tokenHelper', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { '@': { tokenHelper: [osTokenHelper[osFamily]] } },
    })
    expect(result).toStrictEqual({
      authHeaderValueByURI: {
        '//registry.foobar.eu/': 'Bearer token-from-spawn',
      },
      scopedAuthHeaderValueByURI: {},
    })
  })
  it('should prepend Bearer to raw token from tokenHelper', () => {
    const result = getAuthHeadersFromCreds({
      '//registry.foobar.eu/': { '@': { tokenHelper: [osRawTokenHelper[osFamily]] } },
    })
    expect(result).toStrictEqual({
      authHeaderValueByURI: {
        '//registry.foobar.eu/': 'Bearer raw-token-no-scheme',
      },
      scopedAuthHeaderValueByURI: {},
    })
  })
  it('should throw an error if the token helper fails', () => {
    expect(() => getAuthHeadersFromCreds({
      '//reg.com/': { '@': { tokenHelper: [osErrorTokenHelper[osFamily]] } },
    })).toThrow('Exit code')
  })
  it('should throw an error if the token helper returns an empty token', () => {
    expect(() => getAuthHeadersFromCreds({
      '//reg.com/': { '@': { tokenHelper: [osEmptyTokenHelper[osFamily]] } },
    })).toThrow('returned an empty token')
  })
  it('should return empty object when no auth infos', () => {
    const result = getAuthHeadersFromCreds({})
    expect(result).toStrictEqual({
      authHeaderValueByURI: {},
      scopedAuthHeaderValueByURI: {},
    })
  })
  it('should store package scope auth by registry URI and scope', () => {
    const result = getAuthHeadersFromCreds({
      '//npm.pkg.github.com/': {
        '@': { authToken: 'registry-token' },
        '@orgA': { authToken: 'org-a-token' },
        '@orgB': { authToken: 'org-b-token' },
      },
      '//reg.com/npm/': {
        '@orgA': { authToken: 'org-a-path-token' },
      },
    })
    expect(result).toStrictEqual({
      authHeaderValueByURI: {
        '//npm.pkg.github.com/': 'Bearer registry-token',
      },
      scopedAuthHeaderValueByURI: {
        '//npm.pkg.github.com/': {
          '@orgA': 'Bearer org-a-token',
          '@orgB': 'Bearer org-b-token',
        },
        '//reg.com/npm/': {
          '@orgA': 'Bearer org-a-path-token',
        },
      },
    })
    expect(getAuthHeadersByScope(result)).toStrictEqual({
      '//npm.pkg.github.com/': {
        '@': 'Bearer registry-token',
        '@orgA': 'Bearer org-a-token',
        '@orgB': 'Bearer org-b-token',
      },
      '//reg.com/npm/': {
        '@orgA': 'Bearer org-a-path-token',
      },
    })
  })
})
