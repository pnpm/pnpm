import { type Dependencies } from '@pnpm/types'
import { createJsrParamWithoutTag } from '../src/createJsrParam'

const DEPENDENCIES = {
  '@foo/foo': 'jsr:^1.0.0',
  'jsr-bar': 'jsr:@foo/bar@2.0',
  'jsr-baz': 'jsr:@foo/baz',
  '@bar/foo': '^3.0.0',
} satisfies Dependencies

const _createJsrParamWithoutTag: (alias: keyof typeof DEPENDENCIES) => string = createJsrParamWithoutTag.bind(null, DEPENDENCIES)

describe('createJsrParamWithoutTag', () => {
  test('on jsr without alias (jsr:<tag> → jsr:<alias>)', () => {
    expect(_createJsrParamWithoutTag('@foo/foo')).toBe('jsr:@foo/foo')
  })

  test('on jsr with alias (jsr:@<scope>/<name>[@<tag>] → <alias>@jsr:@<scope>/<name>)', () => {
    expect(_createJsrParamWithoutTag('jsr-bar')).toBe('jsr-bar@jsr:@foo/bar')
    expect(_createJsrParamWithoutTag('jsr-baz')).toBe('jsr-baz@jsr:@foo/baz')
  })

  test('on non-jsr', () => {
    expect(_createJsrParamWithoutTag('@bar/foo')).toBe('@bar/foo')
  })
})
