import { parseJsrSpec, parseJsrPackageName } from '../src/parse'
import { type JsrSpec, type ParsedJsrPackageName } from '../src/types'

describe('parseJsrSpec', () => {
  test('skips on non-jsr prefs', () => {
    expect(parseJsrSpec('^1.0.0')).toBeNull()
    expect(parseJsrSpec('1.0.0')).toBeNull()
    expect(parseJsrSpec('latest')).toBeNull()
    expect(parseJsrSpec('npm:foo')).toBeNull()
    expect(parseJsrSpec('npm:@foo/bar')).toBeNull()
    expect(parseJsrSpec('npm:@jsr/foo__bar')).toBeNull()
    expect(parseJsrSpec('catalog:')).toBeNull()
    expect(parseJsrSpec('workspace:*')).toBeNull()
  })

  test('succeeds on jsr prefs that only specify versions/ranges/tags (jsr:<spec>)', () => {
    expect(parseJsrSpec('jsr:^1.0.0')).toStrictEqual({ spec: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpec('jsr:1.0.0')).toStrictEqual({ spec: '1.0.0' } as JsrSpec)
    expect(parseJsrSpec('jsr:latest')).toStrictEqual({ spec: 'latest' } as JsrSpec)
  })

  test('succeeds on jsr prefs that only specify scope and name (jsr:@<scope>/<name>)', () => {
    expect(parseJsrSpec('jsr:@foo/bar')).toStrictEqual({ scope: 'foo', name: 'bar' } as JsrSpec)
  })

  test('succeeds on jsr prefs that specify scopes, names, and versions/ranges/tags (jsr:@<scope>/<name>@<spec>)', () => {
    expect(parseJsrSpec('jsr:@foo/bar@^1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', spec: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpec('jsr:@foo/bar@1.0.0')).toStrictEqual({ scope: 'foo', name: 'bar', spec: '1.0.0' } as JsrSpec)
    expect(parseJsrSpec('jsr:@foo/bar@latest')).toStrictEqual({ scope: 'foo', name: 'bar', spec: 'latest' } as JsrSpec)
  })

  test('errors on jsr prefs that contain names without scopes', () => {
    expect(() => parseJsrSpec('jsr:foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
    }))
  })

  test('errors on jsr prefs that contain scopes without names', () => {
    expect(() => parseJsrSpec('jsr:@foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
    expect(() => parseJsrSpec('jsr:@foo')).toThrow(expect.objectContaining({
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
