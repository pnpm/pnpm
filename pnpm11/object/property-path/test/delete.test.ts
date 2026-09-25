import { expect, test } from '@jest/globals'

import {
  deleteObjectValueByPropertyPathString,
  type UnsafePropertyPathKeyError,
} from '../src/index.js'

test('deletes a top-level key', () => {
  const obj: Record<string, unknown> = { name: 'foo', version: '1.0.0' }
  deleteObjectValueByPropertyPathString(obj, 'version')
  expect(obj).toEqual({ name: 'foo' })
})

test('deletes a nested key', () => {
  const obj: Record<string, unknown> = { scripts: { build: 'tsc', test: 'jest' } }
  deleteObjectValueByPropertyPathString(obj, 'scripts.test')
  expect(obj).toEqual({ scripts: { build: 'tsc' } })
})

test('removes an array element without leaving a hole', () => {
  const obj: Record<string, unknown> = { contributors: [{ name: 'Alice' }, { name: 'Bob' }] }
  deleteObjectValueByPropertyPathString(obj, 'contributors[0]')
  expect(obj).toEqual({ contributors: [{ name: 'Bob' }] })
})

test('removes an array element by string index without leaving a hole', () => {
  const obj: Record<string, unknown> = { contributors: [{ name: 'Alice' }, { name: 'Bob' }] }
  deleteObjectValueByPropertyPathString(obj, 'contributors["0"]')
  expect(obj).toEqual({ contributors: [{ name: 'Bob' }] })
})

test('no-op on a missing path', () => {
  const obj: Record<string, unknown> = { name: 'foo' }
  deleteObjectValueByPropertyPathString(obj, 'scripts.test')
  expect(obj).toEqual({ name: 'foo' })
})

test('no-op when an intermediate value is null', () => {
  const obj: Record<string, unknown> = { a: null }
  deleteObjectValueByPropertyPathString(obj, 'a.b')
  expect(obj).toEqual({ a: null })
})

test('no-op when an intermediate value is a scalar', () => {
  const obj: Record<string, unknown> = { a: 'scalar' }
  deleteObjectValueByPropertyPathString(obj, 'a.b')
  expect(obj).toEqual({ a: 'scalar' })
})

test('no-op on an empty property path', () => {
  const obj: Record<string, unknown> = { name: 'foo' }
  deleteObjectValueByPropertyPathString(obj, '')
  expect(obj).toEqual({ name: 'foo' })
})

test('rejects __proto__, constructor and prototype keys', () => {
  for (const unsafe of ['__proto__', 'constructor', 'prototype']) {
    expect(() => deleteObjectValueByPropertyPathString({}, `${unsafe}.foo`))
      .toThrow(expect.objectContaining({
        code: 'ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY',
        key: unsafe,
      } as Partial<UnsafePropertyPathKeyError>))
  }
})
