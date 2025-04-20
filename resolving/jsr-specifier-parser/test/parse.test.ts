import { parseJsrSpecifier, type JsrSpec } from '@pnpm/resolving.jsr-specifier-parser'

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
    expect(parseJsrSpecifier('jsr:^1.0.0', '@foo/bar')).toStrictEqual({ pref: '^1.0.0', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:1.0.0', '@foo/bar')).toStrictEqual({ pref: '1.0.0', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:latest', '@foo/bar')).toStrictEqual({ pref: 'latest', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
  })

  test('succeeds on jsr prefs that only specify scope and name (jsr:@<scope>/<name>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
  })

  test('succeeds on jsr prefs that specify scopes, names, and versions/ranges/tags (jsr:@<scope>/<name>@<spec>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar@^1.0.0')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', pref: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@1.0.0')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', pref: '1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@latest')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', pref: 'latest' } as JsrSpec)
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
