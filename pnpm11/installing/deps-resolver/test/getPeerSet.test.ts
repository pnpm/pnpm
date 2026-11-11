import { expect, test } from '@jest/globals'
import { resolveFromCatalog } from '@pnpm/catalogs.resolver'

import { getPeerSet } from '../lib/resolvedPackage.js'

const catalogResolver = resolveFromCatalog.bind(null, { default: { foo: '^1.0.0' }, tools: { foo: 'npm:other@1.0.0' } })

test('getPeerSet resolves the catalog ranges of taken peers', () => {
  expect(getPeerSet({
    peerDependencies: { bar: '^2.0.0', foo: 'catalog:' },
  }, catalogResolver)).toStrictEqual({
    declared: { bar: { version: '^2.0.0' }, foo: { version: 'catalog:' } },
    taken: { bar: { version: '^2.0.0' }, foo: { version: '^1.0.0' } },
  })
  expect(getPeerSet({ peerDependencies: { foo: 'catalog:default' } }, catalogResolver).taken)
    .toStrictEqual({ foo: { version: '^1.0.0' } })
  expect(getPeerSet({ peerDependencies: { foo: 'catalog:tools' } }, catalogResolver).taken)
    .toStrictEqual({ foo: { version: 'npm:other@1.0.0' } })
})

test('getPeerSet reports a missing catalog entry of a taken peer', () => {
  expect(() => getPeerSet({ peerDependencies: { foo: 'catalog:missing' } }, catalogResolver))
    .toThrow("No catalog entry 'foo' was found for catalog 'missing'.")
})
