import { expect, test } from '@jest/globals'
import { mergeCatalogs } from '@pnpm/catalogs.config'

test('returns an empty catalog when nothing is passed', () => {
  expect(mergeCatalogs()).toEqual({})
  expect(mergeCatalogs(undefined, undefined)).toEqual({})
})

test('later entries override earlier ones at the dependency level', () => {
  expect(mergeCatalogs(
    {
      default: { foo: '1.0.0', bar: '1.0.0' },
      named: { baz: '1.0.0' },
    },
    {
      default: { foo: '2.0.0' },
    }
  )).toEqual({
    default: { foo: '2.0.0', bar: '1.0.0' },
    named: { baz: '1.0.0' },
  })
})

test('adds catalog entries that did not exist in the base', () => {
  expect(mergeCatalogs(
    { default: { foo: '1.0.0' } },
    { default: { bar: '1.0.0' }, named: { baz: '1.0.0' } }
  )).toEqual({
    default: { foo: '1.0.0', bar: '1.0.0' },
    named: { baz: '1.0.0' },
  })
})

test('skips nullish catalog arguments and nullish named catalogs', () => {
  expect(mergeCatalogs(
    undefined,
    { default: { foo: '1.0.0' }, named: undefined }
  )).toEqual({
    default: { foo: '1.0.0' },
  })
})

test('treats dangerous catalog and dependency names as ordinary own properties', () => {
  // Catalogs parsed from YAML/JSON can carry `__proto__` as an own property,
  // unlike an object literal where `{ __proto__: ... }` sets the prototype.
  const malicious = JSON.parse('{"__proto__":{"polluted":"yes"},"default":{"__proto__":"1.0.0","constructor":"2.0.0"}}')
  const merged = mergeCatalogs(malicious)
  expect(({} as Record<string, unknown>).polluted).toBeUndefined()
  expect(Object.prototype.hasOwnProperty.call(merged, '__proto__')).toBe(true)
  expect(Object.prototype.hasOwnProperty.call(merged.default, '__proto__')).toBe(true)
  expect((merged.default as Record<string, unknown>).__proto__).toBe('1.0.0')
  expect((merged.default as Record<string, unknown>).constructor).toBe('2.0.0')
})
