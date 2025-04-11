import { parseJsrPref, parseJsrPackageName } from '../src/parse'
import { type JsrSpec, type ParsedJsrPackageName } from '../src/types'

describe('parseJsrPref', () => {
  test('skips on non-jsr prefs', () => {
    expect(parseJsrPref('^1.0.0')).toBeNull()
    expect(parseJsrPref('1.0.0')).toBeNull()
    expect(parseJsrPref('latest')).toBeNull()
    expect(parseJsrPref('npm:foo')).toBeNull()
    expect(parseJsrPref('npm:@foo/bar')).toBeNull()
    expect(parseJsrPref('npm:@jsr/foo__bar')).toBeNull()
    expect(parseJsrPref('catalog:')).toBeNull()
    expect(parseJsrPref('workspace:*')).toBeNull()
  })

  test('succeeds on jsr prefs that only specify versions/ranges/tags (jsr:<spec>)', () => {
    expect(parseJsrPref('jsr:^1.0.0')).toStrictEqual({ spec: '^1.0.0' } as JsrSpec)
    expect(parseJsrPref('jsr:1.0.0')).toStrictEqual({ spec: '1.0.0' } as JsrSpec)
    expect(parseJsrPref('jsr:latest')).toStrictEqual({ spec: 'latest' } as JsrSpec)
  })

  test('succeeds on jsr prefs that only specify scope and name (jsr:@<scope>/<name>)', () => {
    expect(parseJsrPref('jsr:@foo/bar')).toStrictEqual({ scope: 'foo', name: 'bar' } as JsrSpec)
  })

  test('succeeds on jsr prefs that specify scopes, names, and versions/ranges/tags (jsr:@<scope>/<name>@<spec>)', () => {
    expect(parseJsrPref('jsr:@foo/bar@^1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', spec: '^1.0.0' } as JsrSpec)
    expect(parseJsrPref('jsr:@foo/bar@1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', spec: '1.0.0' } as JsrSpec)
    expect(parseJsrPref('jsr:@foo/bar@latest')).toStrictEqual({ scope: 'foo', name: 'bar', spec: 'latest' } as JsrSpec)
  })

  test('errors on jsr prefs that contain names without scopes', () => {
    expect(() => parseJsrPref('jsr:foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
    }))
  })

  test('errors on jsr prefs that contain scopes without names', () => {
    expect(() => parseJsrPref('jsr:@foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
    expect(() => parseJsrPref('jsr:@foo')).toThrow(expect.objectContaining({
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
