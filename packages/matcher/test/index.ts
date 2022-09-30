import matcher, { matcherWithIndex } from '@pnpm/matcher'

test('matcher()', () => {
  {
    const match = matcher('*')
    expect(match('@eslint/plugin-foo')).toBe(true)
    expect(match('express')).toBe(true)
  }
  {
    const match = matcher(['eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = matcher(['*plugin*'])
    expect(match('@eslint/plugin-foo')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = matcher(['a*c'])
    expect(match('abc')).toBe(true)
  }
  {
    const match = matcher(['*-positive'])
    expect(match('is-positive')).toBe(true)
  }
  {
    const match = matcher(['foo', 'bar'])
    expect(match('foo')).toBe(true)
    expect(match('bar')).toBe(true)
    expect(match('express')).toBe(false)
  }
  {
    const match = matcher(['eslint-*', '!eslint-plugin-bar'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('eslint-plugin-bar')).toBe(false)
  }
  {
    const match = matcher(['!eslint-plugin-bar', 'eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(true)
    expect(match('eslint-plugin-bar')).toBe(true)
  }
  {
    const match = matcher(['eslint-*', '!eslint-plugin-*', 'eslint-plugin-bar'])
    expect(match('eslint-config-foo')).toBe(true)
    expect(match('eslint-plugin-foo')).toBe(false)
    expect(match('eslint-plugin-bar')).toBe(true)
  }
})

test('matcherWithIndex()', () => {
  {
    const match = matcherWithIndex(['*'])
    expect(match('@eslint/plugin-foo')).toBe(0)
    expect(match('express')).toBe(0)
  }
  {
    const match = matcherWithIndex(['eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('express')).toBe(-1)
  }
  {
    const match = matcherWithIndex(['*plugin*'])
    expect(match('@eslint/plugin-foo')).toBe(0)
    expect(match('express')).toBe(-1)
  }
  {
    const match = matcherWithIndex(['a*c'])
    expect(match('abc')).toBe(0)
  }
  {
    const match = matcherWithIndex(['*-positive'])
    expect(match('is-positive')).toBe(0)
  }
  {
    const match = matcherWithIndex(['foo', 'bar'])
    expect(match('foo')).toBe(0)
    expect(match('bar')).toBe(1)
    expect(match('express')).toBe(-1)
  }
  {
    const match = matcherWithIndex(['eslint-*', '!eslint-plugin-bar'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('eslint-plugin-bar')).toBe(-1)
  }
  {
    const match = matcherWithIndex(['!eslint-plugin-bar', 'eslint-*'])
    expect(match('eslint-plugin-foo')).toBe(0)
    expect(match('eslint-plugin-bar')).toBe(1)
  }
  {
    const match = matcherWithIndex(['eslint-*', '!eslint-plugin-*', 'eslint-plugin-bar'])
    expect(match('eslint-config-foo')).toBe(0)
    expect(match('eslint-plugin-foo')).toBe(-1)
    expect(match('eslint-plugin-bar')).toBe(2)
  }
  {
    const match = matcherWithIndex(['!@pnpm.e2e/peer-*'])
    expect(match('@pnpm.e2e/foo')).toBe(0)
  }
})
