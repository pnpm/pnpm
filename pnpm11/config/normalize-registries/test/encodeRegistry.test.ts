import { expect, test } from '@jest/globals'

import { encodeRegistry } from '../src/encodeRegistry.js'

test('encodeRegistry', () => {
  expect(encodeRegistry('https://registry.npmjs.org/')).toBe('registry.npmjs.org')
  expect(encodeRegistry('https://registry.npmjs.org')).toBe('registry.npmjs.org')
  expect(encodeRegistry('https://npm.example:8443/')).toBe('npm.example+8443')
  expect(encodeRegistry('https://npm.example:443/')).toBe('npm.example')
  expect(encodeRegistry('https://releases.jfrog.io/artifactory/api/npm/coding-agents-npm-a/'))
    .toBe('releases.jfrog.io-artifactory+api+npm+coding-agents-npm-a')
  expect(encodeRegistry('https://releases.jfrog.io/artifactory/api/npm/coding-agents-npm/'))
    .toBe('releases.jfrog.io-artifactory+api+npm+coding-agents-npm')
  expect(encodeRegistry('https://npm.example:8443/registry/A/'))
    .toBe('npm.example+8443-registry+A')
  expect(encodeRegistry('https://npm.example/path/a+b/'))
    .toBe('npm.example-path+a%2Bb')
  expect(encodeRegistry('https://npm.example/path/a:b/'))
    .toBe('npm.example-path+a%3Ab')
  expect(encodeRegistry('https://npm.example/path/a%2Bb/'))
    .toBe('npm.example-path+a%252Bb')
  expect(() => encodeRegistry('invalid-url')).toThrow('Failed to parse registry URL')
})

