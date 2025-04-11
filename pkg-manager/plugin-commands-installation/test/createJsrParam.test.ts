import { type Dependencies } from '@pnpm/types'
import { createJsrParamWithoutSpec } from '../src/createJsrParam'

const DEPENDENCIES = {
  '@foo/foo': 'jsr:^1.0.0',
  'jsr-bar': 'jsr:@foo/bar@2.0',
  'jsr-baz': 'jsr:@foo/baz',
  '@bar/foo': '^3.0.0',
} satisfies Dependencies

const _createJsrParamWithoutSpec: (alias: keyof typeof DEPENDENCIES) => string = createJsrParamWithoutSpec.bind(null, DEPENDENCIES)

describe('createJsrParamWithoutSpec', () => {
  test('on jsr without alias (jsr:<spec> → jsr:<alias>)', () => {
    expect(_createJsrParamWithoutSpec('@foo/foo')).toBe('jsr:@foo/foo')
  })

  test('on jsr with alias (jsr:@<scope>/<name>[@<spec>] → <alias>@jsr:@<scope>/<name>)', () => {
    expect(_createJsrParamWithoutSpec('jsr-bar')).toBe('jsr-bar@jsr:@foo/bar')
    expect(_createJsrParamWithoutSpec('jsr-baz')).toBe('jsr-baz@jsr:@foo/baz')
  })

  test('on non-jsr', () => {
    expect(_createJsrParamWithoutSpec('@bar/foo')).toBe('@bar/foo')
  })
})
