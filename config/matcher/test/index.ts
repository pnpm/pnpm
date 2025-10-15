import { createMatcher, createMatcherWithIndex, createVersionMatcher } from '@pnpm/matcher'

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

test('createVersionMatcher()', () => {
  {
    const match = createVersionMatcher(['axios@1.12.2'])
    expect(match('axios', '1.12.2')).toBe(true)
    expect(match('axios', '1.12.3')).toBe(false)
    expect(match('axios', '1.12.1')).toBe(false)
  }
  {
    const match = createVersionMatcher(['is-*'])
    expect(match('is-odd', '0.1.2')).toBe(true)
    expect(match('is-even', '1.0.0')).toBe(true)
    expect(match('lodash', '4.17.21')).toBe(false)
  }
  {
    const match = createVersionMatcher(['@babel/core@7.20.0'])
    expect(match('@babel/core', '7.20.0')).toBe(true)
    expect(match('@babel/core', '7.20.1')).toBe(false)
  }
  {
    const match = createVersionMatcher(['@babel/core'])
    expect(match('@babel/core', '7.20.0')).toBe(true)
    expect(match('@babel/core', '6.26.0')).toBe(true)
  }
  {
    const match = createVersionMatcher(['axios@1.12.2'])
    expect(match('axios')).toBe(false)
  }
  {
    const match = createVersionMatcher(['axios@1.12.2', 'lodash@4.17.21', 'is-*'])
    expect(match('axios', '1.12.2')).toBe(true)
    expect(match('axios', '1.12.3')).toBe(false)
    expect(match('lodash', '4.17.21')).toBe(true)
    expect(match('is-odd', '0.1.2')).toBe(true)
  }
  {
    expect(() => createVersionMatcher(['lodash@^4.17.0'])).toThrow(/Semantic version ranges are not supported/)
    expect(() => createVersionMatcher(['lodash@~4.17.0'])).toThrow(/Semantic version ranges are not supported/)
    expect(() => createVersionMatcher(['react@>=18.0.0'])).toThrow(/Semantic version ranges are not supported/)
  }
  {
    const match = createVersionMatcher(['axios@1.12.0 || 1.12.1'])
    expect(match('axios', '1.12.0')).toBe(true)
    expect(match('axios', '1.12.1')).toBe(true)
    expect(match('axios', '1.12.2')).toBe(false)
  }
  {
    const match = createVersionMatcher(['@scope/pkg@1.0.0 || 1.0.1'])
    expect(match('@scope/pkg', '1.0.0')).toBe(true)
    expect(match('@scope/pkg', '1.0.1')).toBe(true)
    expect(match('@scope/pkg', '1.0.2')).toBe(false)
  }
  {
    const match = createVersionMatcher(['pkg@1.0.0||1.0.1  ||  1.0.2'])
    expect(match('pkg', '1.0.0')).toBe(true)
    expect(match('pkg', '1.0.1')).toBe(true)
    expect(match('pkg', '1.0.2')).toBe(true)
    expect(match('pkg', '1.0.3')).toBe(false)
  }
})
