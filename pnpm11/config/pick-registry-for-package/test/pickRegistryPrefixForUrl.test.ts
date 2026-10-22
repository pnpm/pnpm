import { expect, test } from '@jest/globals'
import { pickRegistryPrefixForUrl } from '@pnpm/config.pick-registry-for-package'

const WORK = 'https://npm.work.example.com/'

test('a registry matches its prefix regardless of trailing slash and host case', () => {
  const registriesByPrefix = { work: 'https://NPM.Work.Example.com' }
  expect(pickRegistryPrefixForUrl(registriesByPrefix, WORK)).toBe('work')
  expect(pickRegistryPrefixForUrl(registriesByPrefix, 'HTTPS://npm.work.example.com')).toBe('work')
  expect(pickRegistryPrefixForUrl(registriesByPrefix, 'https://npm.other.example.com/')).toBeUndefined()
})

test('a registry path keeps its case', () => {
  const registriesByPrefix = { work: 'https://registry.example.com/Work/' }
  expect(pickRegistryPrefixForUrl(registriesByPrefix, 'https://registry.example.com/work/')).toBeUndefined()
  expect(pickRegistryPrefixForUrl(registriesByPrefix, 'https://registry.example.com/Work')).toBe('work')
})

test('the alphabetically first of several prefixes on one registry is answered', () => {
  expect(pickRegistryPrefixForUrl({ zeta: WORK, alpha: WORK, mid: WORK }, WORK)).toBe('alpha')
})

test('no declared prefix answers nothing', () => {
  expect(pickRegistryPrefixForUrl(undefined, 'https://registry.npmjs.org/')).toBeUndefined()
  expect(pickRegistryPrefixForUrl({}, 'https://registry.npmjs.org/')).toBeUndefined()
})
