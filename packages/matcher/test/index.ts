import { createMatcher, createMatcherWithIndex } from '@pnpm/matcher'

test('matcher()', () => {
  {
    const match = createMatcher('*')
    expect(match('@eslint/plugin-foo')).toBe(true)
    expect(match('express')).toBe(true)
  }
  {
    const match = createMatcher(['eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = createMatcher(['*plugin*'])
    expect(match('@eslint/plugin-foo')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = createMatcher(['a*c'])
    expect(match('abc')).toBe(true)
  }
  {
    const match = createMatcher(['*-positive'])
    expect(match('is-positive')).toBe(true)
  }
  {
    const match = createMatcher(['foo', 'bar'])
    expect(match('foo')).toBe(true)
    expect(match('bar')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = createMatcher(['eslint-*', '!eslint-plugin-bar'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('eslint-plugin-bar')).toBe(false)
  }
  {
    const match = createMatcher(['!eslint-plugin-bar', 'eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('eslint-plugin-bar')).toBe(true)
  }
  {
    const match = createMatcher(['eslint-*', '!eslint-plugin-*', 'eslint-plugin-bar'])
    expect(match('eslint-config-foo')).toBe(true)
    expect(match('eslint-plugin-foo')).toBe(false)
    expect(match('eslint-plugin-bar')).toBe(true)
  }
})

test('createMatcherWithIndex()', () => {
  {
    const match = createMatcherWithIndex(['*'])
    expect(match('@eslint/plugin-foo')).toBe(0)
    expect(match('express')).toBe(0)
  }
  {
    const match = createMatcherWithIndex(['eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('express')).toBe(-1)
  }
  {
    const match = createMatcherWithIndex(['*plugin*'])
    expect(match('@eslint/plugin-foo')).toBe(0)
    expect(match('express')).toBe(-1)
  }
  {
    const match = createMatcherWithIndex(['a*c'])
    expect(match('abc')).toBe(0)
  }
  {
    const match = createMatcherWithIndex(['*-positive'])
    expect(match('is-positive')).toBe(0)
  }
  {
    const match = createMatcherWithIndex(['foo', 'bar'])
    expect(match('foo')).toBe(0)
    expect(match('bar')).toBe(1)
    expect(match('express')).toBe(-1)
  }
  {
    const match = createMatcherWithIndex(['eslint-*', '!eslint-plugin-bar'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('eslint-plugin-bar')).toBe(-1)
  }
  {
    const match = createMatcherWithIndex(['!eslint-plugin-bar', 'eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('eslint-plugin-bar')).toBe(1)
  }
  {
    const match = createMatcherWithIndex(['eslint-*', '!eslint-plugin-*', 'eslint-plugin-bar'])
    expect(match('eslint-config-foo')).toBe(0)
    expect(match('eslint-plugin-foo')).toBe(-1)
    expect(match('eslint-plugin-bar')).toBe(2)
  }
  {
    const match = createMatcherWithIndex(['!@pnpm.e2e/peer-*'])
    expect(match('@pnpm.e2e/foo')).toBe(0)
  }
})
