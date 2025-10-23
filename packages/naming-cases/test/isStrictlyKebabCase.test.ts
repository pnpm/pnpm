import { isStrictlyKebabCase } from '../src/index.js'

test('kebab-case names with more than 1 words should satisfy', () => {
  expect(isStrictlyKebabCase('foo-bar')).toBe(true)
  expect(isStrictlyKebabCase('foo-bar123')).toBe(true)
  expect(isStrictlyKebabCase('a123-foo')).toBe(true)
})

test('names with uppercase letters should not satisfy', () => {
  expect(isStrictlyKebabCase('foo-Bar')).toBe(false)
  expect(isStrictlyKebabCase('Foo-Bar')).toBe(false)
  expect(isStrictlyKebabCase('Foo-bar')).toBe(false)
})

test('names with underscores should not satisfy', () => {
  expect(isStrictlyKebabCase('foo_bar')).toBe(false)
  expect(isStrictlyKebabCase('foo-bar_baz')).toBe(false)
  expect(isStrictlyKebabCase('_foo-bar')).toBe(false)
})

test('names with only 1 word should not satisfy', () => {
  expect(isStrictlyKebabCase('foo')).toBe(false)
  expect(isStrictlyKebabCase('bar')).toBe(false)
  expect(isStrictlyKebabCase('a123')).toBe(false)
})

test('names that start with a number should not satisfy', () => {
  expect(isStrictlyKebabCase('123a')).toBe(false)
})

test('names with two or more dashes next to each other should not satisfy', () => {
  expect(isStrictlyKebabCase('foo--bar')).toBe(false)
  expect(isStrictlyKebabCase('foo-bar--baz')).toBe(false)
})

test('names that start or end with a dash should not satisfy', () => {
  expect(isStrictlyKebabCase('-foo-bar')).toBe(false)
  expect(isStrictlyKebabCase('foo-bar-')).toBe(false)
})

test('names with special characters should not satisfy', () => {
  expect(isStrictlyKebabCase('foo@bar')).toBe(false)
})
