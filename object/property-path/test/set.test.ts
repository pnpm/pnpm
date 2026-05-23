import { expect, test } from '@jest/globals'

import {
  EmptyPropertyPathError,
  setObjectValueByPropertyPathString,
  UnsafePropertyPathKeyError,
} from '../src/index.js'

test('sets a top-level key', () => {
  const obj: Record<string, unknown> = { name: 'foo' }
  setObjectValueByPropertyPathString(obj, 'version', '1.0.0')
  expect(obj).toEqual({ name: 'foo', version: '1.0.0' })
})

test('creates missing intermediate objects', () => {
  const obj: Record<string, unknown> = {}
  setObjectValueByPropertyPathString(obj, 'scripts.build', 'tsc')
  expect(obj).toEqual({ scripts: { build: 'tsc' } })
})

test('creates an array when the next segment is numeric', () => {
  const obj: Record<string, unknown> = {}
  setObjectValueByPropertyPathString(obj, 'contributors[0].name', 'Alice')
  expect(obj).toEqual({ contributors: [{ name: 'Alice' }] })
})

test('replaces a scalar intermediate with the right container', () => {
  const obj: Record<string, unknown> = { scripts: 'echo hi' }
  setObjectValueByPropertyPathString(obj, 'scripts.test', 'vitest')
  expect(obj).toEqual({ scripts: { test: 'vitest' } })
})

test('replaces a scalar intermediate with an array when the next segment is numeric', () => {
  const obj: Record<string, unknown> = { keywords: 'oops' }
  setObjectValueByPropertyPathString(obj, 'keywords[0]', 'pnpm')
  expect(obj).toEqual({ keywords: ['pnpm'] })
})

test('replaces an array intermediate with an object when the next segment is a string', () => {
  const obj: Record<string, unknown> = { contributors: [] }
  setObjectValueByPropertyPathString(obj, 'contributors.name', 'Alice')
  expect(obj).toEqual({ contributors: { name: 'Alice' } })
})

test('replaces an object intermediate with an array when the next segment is numeric', () => {
  const obj: Record<string, unknown> = { contributors: { x: 1 } }
  setObjectValueByPropertyPathString(obj, 'contributors[0]', 'Alice')
  expect(obj).toEqual({ contributors: ['Alice'] })
})

test('overwrites an existing value', () => {
  const obj: Record<string, unknown> = { scripts: { build: 'old' } }
  setObjectValueByPropertyPathString(obj, 'scripts.build', 'new')
  expect(obj).toEqual({ scripts: { build: 'new' } })
})

test('rejects __proto__, constructor and prototype keys', () => {
  for (const unsafe of ['__proto__', 'constructor', 'prototype']) {
    expect(() => setObjectValueByPropertyPathString({}, `${unsafe}.polluted`, true))
      .toThrow(expect.objectContaining({
        code: 'ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY',
        key: unsafe,
      } as Partial<UnsafePropertyPathKeyError>))
  }
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  expect(({} as any).polluted).toBeUndefined()
})

test('throws on empty property path', () => {
  expect(() => setObjectValueByPropertyPathString({}, '', 'value')).toThrow(EmptyPropertyPathError)
})
