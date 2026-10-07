import { expect, test } from '@jest/globals'
import { isLoopbackHost, isUrlSecureForCredentials, nerfDart } from '@pnpm/config.registry-auth-key'

test.each([
  ['//registry.npmjs.org/', [
    'https://registry.npmjs.org',
    'https://registry.npmjs.org/package-name',
    'https://registry.npmjs.org/package-name?write=true',
    'https://registry.npmjs.org/@scope%2fpackage-name',
    'https://registry.npmjs.org/@scope%2fpackage-name?write=true',
    'https://username:password@registry.npmjs.org/package-name?write=true',
    'https://registry.npmjs.org/#hash',
    'https://registry.npmjs.org/?write=true#hash',
    'https://registry.npmjs.org/package-name?write=true#hash',
    'https://registry.npmjs.org/package-name#hash',
    'https://registry.npmjs.org/@scope%2fpackage-name?write=true#hash',
    'https://registry.npmjs.org/@scope%2fpackage-name#hash',
  ]],
  ['//my-couch:5984/registry/_design/app/rewrite/', [
    'https://my-couch:5984/registry/_design/app/rewrite/',
    'https://my-couch:5984/registry/_design/app/rewrite/package-name',
    'https://my-couch:5984/registry/_design/app/rewrite/package-name?write=true',
    'https://my-couch:5984/registry/_design/app/rewrite/@scope%2fpackage-name',
    'https://my-couch:5984/registry/_design/app/rewrite/@scope%2fpackage-name?write=true',
    'https://username:password@my-couch:5984/registry/_design/app/rewrite/package-name?write=true',
    'https://my-couch:5984/registry/_design/app/rewrite/#hash',
    'https://my-couch:5984/registry/_design/app/rewrite/?write=true#hash',
    'https://my-couch:5984/registry/_design/app/rewrite/package-name?write=true#hash',
    'https://my-couch:5984/registry/_design/app/rewrite/package-name#hash',
    'https://my-couch:5984/registry/_design/app/rewrite/@scope%2fpackage-name?write=true#hash',
    'https://my-couch:5984/registry/_design/app/rewrite/@scope%2fpackage-name#hash',
  ]],
])('nerfDart() returns %s', (key, urls) => {
  expect.assertions(urls.length)
  for (const url of urls) {
    expect(nerfDart(url)).toBe(key)
  }
})

test('nerfDart() throws on an invalid URL', () => {
  expect(() => nerfDart('not a valid url')).toThrow()
})

test('isUrlSecureForCredentials()', () => {
  expect(isUrlSecureForCredentials('https://registry.npmjs.org/')).toBe(true)
  expect(isUrlSecureForCredentials('https://example.com/some/path')).toBe(true)
  expect(isUrlSecureForCredentials('http://localhost/')).toBe(true)
  expect(isUrlSecureForCredentials('http://localhost:4873/')).toBe(true)
  expect(isUrlSecureForCredentials('http://127.0.0.1/')).toBe(true)
  expect(isUrlSecureForCredentials('http://127.0.0.1:4873/')).toBe(true)
  expect(isUrlSecureForCredentials('http://[::1]/')).toBe(true)
  expect(isUrlSecureForCredentials('http://[::1]:4873/')).toBe(true)
  expect(isUrlSecureForCredentials('http://127.1.2.3:8080/')).toBe(true)

  expect(isUrlSecureForCredentials('http://registry.npmjs.org/')).toBe(false)
  expect(isUrlSecureForCredentials('http://example.com/')).toBe(false)
  expect(isUrlSecureForCredentials('http://127.attacker.example/')).toBe(false)
  expect(isUrlSecureForCredentials('ftp://localhost/')).toBe(false)
  expect(isUrlSecureForCredentials('not a valid url')).toBe(false)
})

test('isLoopbackHost()', () => {
  expect(isLoopbackHost('localhost')).toBe(true)
  expect(isLoopbackHost('LOCALHOST')).toBe(true)
  expect(isLoopbackHost('127.0.0.1')).toBe(true)
  expect(isLoopbackHost('::1')).toBe(true)
  expect(isLoopbackHost('[::1]')).toBe(true)
  expect(isLoopbackHost('127.0.0.2')).toBe(true)

  expect(isLoopbackHost('example.com')).toBe(false)
  expect(isLoopbackHost('127.attacker.example')).toBe(false)
  expect(isLoopbackHost('not-an-ip-and-not-localhost')).toBe(false)
  expect(isLoopbackHost('::2')).toBe(false)
})


