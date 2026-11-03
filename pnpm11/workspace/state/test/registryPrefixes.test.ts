import { expect, test } from '@jest/globals'

import { recordedRegistryKeySettings, recordedRegistryPrefixes, recordedRegistryRoutes } from '../src/registryPrefixes.js'

const WORK = 'https://npm.work.test/'

test('recordedRegistryPrefixes() records nothing when no prefix is declared', () => {
  expect(recordedRegistryPrefixes({})).toBeUndefined()
  expect(recordedRegistryPrefixes({ registriesByPrefix: {} })).toBeUndefined()
})

test('recordedRegistryPrefixes() records the declared prefixes in sorted order', () => {
  expect(recordedRegistryPrefixes({ registriesByPrefix: { zeta: WORK, work: WORK } })).toStrictEqual({ work: WORK, zeta: WORK })
  expect(Object.keys(recordedRegistryPrefixes({ registriesByPrefix: { zeta: WORK, work: WORK } })!)).toStrictEqual(['work', 'zeta'])
})

test('recordedRegistryRoutes() records the scope routes in sorted order, without the default registry', () => {
  const routes = recordedRegistryRoutes({ default: 'https://registry.npmjs.org/', '@corp': WORK, '@aaa': WORK })
  expect(Object.keys(routes)).toStrictEqual(['@aaa', '@corp'])
  expect(routes).toStrictEqual({ '@aaa': WORK, '@corp': WORK })
})

test('recordedRegistryKeySettings() records nothing when no prefix is declared', () => {
  expect(recordedRegistryKeySettings({ registriesByScope: { default: 'https://registry.npmjs.org/' } })).toStrictEqual({})
})

test('recordedRegistryKeySettings() records the default registry with the declared prefixes', () => {
  expect(recordedRegistryKeySettings({
    registriesByPrefix: { work: WORK },
    registriesByScope: { default: 'HTTPS://Registry.npmjs.org' },
  })).toStrictEqual({ registryPrefixes: { work: WORK }, defaultRegistry: 'https://registry.npmjs.org/' })
})
