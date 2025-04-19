import { parseJsrSpecifier, parseJsrPackageName } from '../src/parse'
import { type JsrSpec, type ParsedJsrPackageName } from '../src/types'

describe('parseJsrSpecifier', () => {
  test('skips on non-jsr prefs', () => {
    expect(parseJsrSpecifier('^1.0.0')).toBeNull()
    expect(parseJsrSpecifier('1.0.0')).toBeNull()
    expect(parseJsrSpecifier('latest')).toBeNull()
    expect(parseJsrSpecifier('npm:foo')).toBeNull()
    expect(parseJsrSpecifier('npm:@foo/bar')).toBeNull()
    expect(parseJsrSpecifier('npm:@jsr/foo__bar')).toBeNull()
    expect(parseJsrSpecifier('catalog:')).toBeNull()
    expect(parseJsrSpecifier('workspace:*')).toBeNull()
  })

  test('succeeds on jsr prefs that only specify versions/ranges/tags (jsr:<spec>)', () => {
    expect(parseJsrSpecifier('jsr:^1.0.0')).toStrictEqual({ pref: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:1.0.0')).toStrictEqual({ pref: '1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:latest')).toStrictEqual({ pref: 'latest' } as JsrSpec)
  })

  test('succeeds on jsr prefs that only specify scope and name (jsr:@<scope>/<name>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar')).toStrictEqual({ scope: 'foo', name: 'bar' } as JsrSpec)
  })

  test('succeeds on jsr prefs that specify scopes, names, and versions/ranges/tags (jsr:@<scope>/<name>@<spec>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar@^1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', pref: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', pref: '1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@latest')).toStrictEqual({ scope: 'foo', name: 'bar', pref: 'latest' } as JsrSpec)
  })

  test('errors on jsr prefs that contain names without scopes', () => {
    expect(() => parseJsrSpecifier('jsr:foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
    }))
  })

  test('errors on jsr prefs that contain scopes without names', () => {
    expect(() => parseJsrSpecifier('jsr:@foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
    expect(() => parseJsrSpecifier('jsr:@foo')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
  })
})

describe('parseJsrPackageName', () => {
  test('succeeds on names with scopes', () => {
    expect(parseJsrPackageName('@foo/bar')).toStrictEqual({ scope: 'foo', name: 'bar' } as ParsedJsrPackageName)
  })

  test('errors on names without scopes', () => {
    expect(() => parseJsrPackageName('bar')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
    }))
  })

  test('errors on scopes without names', () => {
    expect(() => parseJsrPackageName('@foo')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
  })
})
