import { isCamelCase } from '../src/index.js'

test('camelCase names should satisfy', () => {
  expect(isCamelCase('foo')).toBe(true)
  expect(isCamelCase('fooBar')).toBe(true)
  expect(isCamelCase('fooBarBaz')).toBe(true)
  expect(isCamelCase('foo123')).toBe(true)
  expect(isCamelCase('fooBar123')).toBe(true)
  expect(isCamelCase('fooBarBaz123')).toBe(true)
  expect(isCamelCase('aBcDef')).toBe(true)
})

test('names that start with uppercase letter should not satisfy', () => {
  expect(isCamelCase('Foo')).toBe(false)
  expect(isCamelCase('FooBar')).toBe(false)
  expect(isCamelCase('FooBarBaz')).toBe(false)
  expect(isCamelCase('Foo123')).toBe(false)
  expect(isCamelCase('FooBar123')).toBe(false)
  expect(isCamelCase('FooBarBaz123')).toBe(false)
  expect(isCamelCase('ABcDef')).toBe(false)
})

test('names with hyphens and/or underscores should not satisfy', () => {
  expect(isCamelCase('foo-bar')).toBe(false)
  expect(isCamelCase('foo-Bar')).toBe(false)
  expect(isCamelCase('foo-bar-baz')).toBe(false)
  expect(isCamelCase('foo-Bar-Baz')).toBe(false)
  expect(isCamelCase('foo_bar')).toBe(false)
  expect(isCamelCase('foo_Bar')).toBe(false)
  expect(isCamelCase('foo_bar_baz')).toBe(false)
  expect(isCamelCase('foo_Bar_Baz')).toBe(false)
  expect(isCamelCase('foo-bar')).toBe(false)
  expect(isCamelCase('foo-Bar')).toBe(false)
  expect(isCamelCase('foo-bar_baz')).toBe(false)
  expect(isCamelCase('foo-Bar_Baz')).toBe(false)
  expect(isCamelCase('_foo')).toBe(false)
  expect(isCamelCase('foo_')).toBe(false)
  expect(isCamelCase('-foo')).toBe(false)
  expect(isCamelCase('foo-')).toBe(false)
})

test('names that start with a number should not satisfy', () => {
  expect(isCamelCase('123a')).toBe(false)
})

test('names with special characters should not satisfy', () => {
  expect(isCamelCase('foo@bar')).toBe(false)
})
