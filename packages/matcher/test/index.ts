import matcher from '@pnpm/matcher'

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
