import path from 'path'
import os from 'os'
import { getAuthHeadersFromConfig } from '../src/getAuthHeadersFromConfig'
import { Buffer } from 'safe-buffer'

const osTokenHelper = {
  linux: path.join(__dirname, 'utils/test-exec.js'),
  win32: path.join(__dirname, 'utils/test-exec.bat'),
}

const osErrorTokenHelper = {
  linux: path.join(__dirname, 'utils/test-exec-error.js'),
  win32: path.join(__dirname, 'utils/test-exec-error.bat'),
}

// Only exception is win32, all others behave like linux
const osFamily = os.platform() === 'win32' ? 'win32' : 'linux'

describe('getAuthHeadersFromConfig()', () => {
  it('should get settings', () => {
    const allSettings = {
      '//registry.npmjs.org/:_authToken': 'abc123',
      '//registry.foobar.eu/:_password': encodeBase64('foobar'),
      '//registry.foobar.eu/:username': 'foobar',
      '//registry.hu/:_auth': 'foobar',
      '//localhost:3000/:_auth': 'foobar',
    }
    const userSettings = {}
    expect(getAuthHeadersFromConfig({ allSettings, userSettings })).toStrictEqual({
      '//registry.npmjs.org/': 'Bearer abc123',
      '//registry.foobar.eu/': 'Basic Zm9vYmFyOmZvb2Jhcg==',
      '//registry.hu/': 'Basic foobar',
      '//localhost:3000/': 'Basic foobar',
    })
  })
  describe('should get settings for the default registry', () => {
    it('_auth', () => {
      const allSettings = {
        registry: 'https://reg.com/',
        _auth: 'foobar',
      }
      expect(getAuthHeadersFromConfig({ allSettings, userSettings: {} })).toStrictEqual({
        '//reg.com/': 'Basic foobar',
      })
    })
    it('username/_password', () => {
      const allSettings = {
        registry: 'https://reg.com/',
        username: 'foo',
        _password: 'bar',
      }
      expect(getAuthHeadersFromConfig({ allSettings, userSettings: {} })).toStrictEqual({
        '//reg.com/': `Basic ${encodeBase64('foo:bar')}`,
      })
    })
    it('tokenHelper', () => {
      const allSettings = {
        registry: 'https://reg.com/',
      }
      const userSettings = {
        tokenHelper: osTokenHelper[osFamily],
      }
      expect(getAuthHeadersFromConfig({ allSettings, userSettings })).toStrictEqual({
        '//reg.com/': 'Bearer token-from-spawn',
      })
    })
    it('only read token helper from user config', () => {
      const allSettings = {
        registry: 'https://reg.com/',
        tokenHelper: osTokenHelper[osFamily],
      }
      expect(getAuthHeadersFromConfig({ allSettings, userSettings: {} })).toStrictEqual({})
    })
  })
  it('should get tokenHelper', () => {
    const userSettings = {
      '//registry.foobar.eu/:tokenHelper': osTokenHelper[osFamily],
    }
    expect(getAuthHeadersFromConfig({ allSettings: {}, userSettings })).toStrictEqual({
      '//registry.foobar.eu/': 'Bearer token-from-spawn',
    })
  })
  it('should throw an error if the token helper is not an absolute path', () => {
    expect(() => getAuthHeadersFromConfig({
      allSettings: {},
      userSettings: {
        '//reg.com:tokenHelper': './utils/text-exec.js',
      },
    })).toThrowError('must be an absolute path, without arguments')
  })
  it('should throw an error if the token helper is not an absolute path with args', () => {
    expect(() => getAuthHeadersFromConfig({
      allSettings: {},
      userSettings: {
        '//reg.com:tokenHelper': `${osTokenHelper[osFamily]} arg1`,
      },
    })).toThrowError('must be an absolute path, without arguments')
  })
  it('should throw an error if the token helper fails', () => {
    expect(() => getAuthHeadersFromConfig({
      allSettings: {},
      userSettings: {
        '//reg.com:tokenHelper': osErrorTokenHelper[osFamily],
      },
    })).toThrowError('Exit code')
  })
  it('only read token helper from user config', () => {
    const allSettings = {
      '//reg.com:tokenHelper': osTokenHelper[osFamily],
    }
    expect(getAuthHeadersFromConfig({ allSettings, userSettings: {} })).toStrictEqual({})
  })
})

function encodeBase64 (s: string) {
  return Buffer.from(s, 'utf8').toString('base64')
}
