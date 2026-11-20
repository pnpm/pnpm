import { expect, test } from '@jest/globals'
import type { RegistryContext } from '@pnpm/types'

import { lockedVersionSatisfies } from '../lib/dependencyManifest.js'

const registries: RegistryContext = {
  registriesByScope: { default: 'https://registry.npmjs.org/' },
  registriesByPrefix: { work: 'https://npm.work.example.com/' },
}

test('lockedVersionSatisfies() checks the package a named-registry spec names', () => {
  const locked = { name: 'bar', version: '1.2.0', registryName: 'work' }
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:bar@^1.0.0' }, locked, registries)).toBe(true)
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:baz@^1.0.0' }, locked, registries)).toBe(false)
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:^1.0.0' }, locked, registries)).toBe(false)
  expect(lockedVersionSatisfies({ alias: 'bar', bareSpecifier: 'work:^1.0.0' }, locked, registries)).toBe(true)
})

test('lockedVersionSatisfies() cannot judge a spec that is not a range', () => {
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'latest' }, { name: 'foo', version: '1.0.0' }, registries)).toBeUndefined()
})
