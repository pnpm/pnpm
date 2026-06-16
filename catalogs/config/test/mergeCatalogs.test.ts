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
