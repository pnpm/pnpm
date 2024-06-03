import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'

test('parses named catalog', () => {
  expect(parseCatalogProtocol('catalog:foo')).toBe('foo')
  expect(parseCatalogProtocol('catalog:bar')).toBe('bar')
})

test('returns null for specifier not using catalog protocol', () => {
  expect(parseCatalogProtocol('^1.0.0')).toBe(null)
})

describe('default catalog', () => {
  test('parses explicit default catalog', () => {
    expect(parseCatalogProtocol('catalog:default')).toBe('default')
  })

  test('parses implicit catalog', () => {
    expect(parseCatalogProtocol('catalog:')).toBe('default')
  })
})
