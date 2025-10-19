import { createMatcher, createMatcherWithIndex, createPackageVersionPolicy } from '@pnpm/matcher'

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
    expect(match('eslint-plugin-foo')).toBe(1)
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
  {
    const match = createMatcherWithIndex(['!foo', '!bar'])
    expect(match('foo')).toBe(-1)
    expect(match('bar')).toBe(-1)
    expect(match('baz')).toBe(0)
  }
  {
    const match = createMatcherWithIndex(['!foo', '!bar', 'qar'])
    expect(match('foo')).toBe(-1)
    expect(match('bar')).toBe(-1)
    expect(match('baz')).toBe(-1)
  }
})

test('createPackageVersionPolicy()', () => {
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
  }
  {
    const match = createPackageVersionPolicy(['is-*'])
    expect(match('is-odd')).toBe(true)
    expect(match('is-even')).toBe(true)
    expect(match('lodash')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['@babel/core@7.20.0'])
    expect(match('@babel/core')).toStrictEqual(['7.20.0'])
  }
  {
    const match = createPackageVersionPolicy(['@babel/core'])
    expect(match('@babel/core')).toBe(true)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('is-odd')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2', 'lodash@4.17.21', 'is-*'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
    expect(match('lodash')).toStrictEqual(['4.17.21'])
    expect(match('is-odd')).toBe(true)
  }
  {
    expect(() => createPackageVersionPolicy(['lodash@^4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['lodash@~4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['react@>=18.0.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['is-*@1.0.0'])).toThrow(/Name patterns are not allowed/)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.0 || 1.12.1'])
    expect(match('axios')).toStrictEqual(['1.12.0', '1.12.1'])
  }
  {
    const match = createPackageVersionPolicy(['@scope/pkg@1.0.0 || 1.0.1'])
    expect(match('@scope/pkg')).toStrictEqual(['1.0.0', '1.0.1'])
  }
  {
    const match = createPackageVersionPolicy(['pkg@1.0.0||1.0.1  ||  1.0.2'])
    expect(match('pkg')).toStrictEqual(['1.0.0', '1.0.1', '1.0.2'])
  }
})
