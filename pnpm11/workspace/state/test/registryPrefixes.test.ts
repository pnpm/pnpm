import { expect, test } from '@jest/globals'

import { recordedRegistryPrefixes } from '../src/registryPrefixes.js'

const WORK = 'https://npm.work.test/'

test('recordedRegistryPrefixes() records nothing when no prefix is declared', () => {
  expect(recordedRegistryPrefixes({ registriesByScope: { default: WORK } })).toBeUndefined()
  expect(recordedRegistryPrefixes({ registriesByPrefix: {}, registriesByScope: { default: WORK } })).toBeUndefined()
})

test('recordedRegistryPrefixes() records the routes that reach a registry declaring a prefix', () => {
  expect(recordedRegistryPrefixes({
    registriesByPrefix: { work: WORK },
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@corp': 'https://npm.work.test',
    },
  })).toStrictEqual({
    'route:@corp': 'work',
    work: WORK,
  })
})

test('recordedRegistryPrefixes() changes when a scope moves to a registry declaring a prefix', () => {
  const registriesByPrefix = { work: WORK }
  expect(recordedRegistryPrefixes({ registriesByPrefix, registriesByScope: { default: 'https://registry.npmjs.org/' } }))
    .not.toStrictEqual(recordedRegistryPrefixes({ registriesByPrefix, registriesByScope: { default: 'https://registry.npmjs.org/', '@corp': WORK } }))
})
