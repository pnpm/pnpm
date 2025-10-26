import { parseJsrSpecifier, type JsrSpec } from '@pnpm/resolving.jsr-specifier-parser'

describe('parseJsrSpecifier', () => {
  test('skips on non-jsr specifiers', () => {
    expect(parseJsrSpecifier('^1.0.0')).toBeNull()
    expect(parseJsrSpecifier('1.0.0')).toBeNull()
    expect(parseJsrSpecifier('latest')).toBeNull()
    expect(parseJsrSpecifier('npm:foo')).toBeNull()
    expect(parseJsrSpecifier('npm:@foo/bar')).toBeNull()
    expect(parseJsrSpecifier('npm:@jsr/foo__bar')).toBeNull()
    expect(parseJsrSpecifier('catalog:')).toBeNull()
    expect(parseJsrSpecifier('workspace:*')).toBeNull()
  })

  test('succeeds on jsr specifiers that only specify versions/ranges/tags (jsr:<version_selector>)', () => {
    expect(parseJsrSpecifier('jsr:^1.0.0', '@foo/bar')).toStrictEqual({ versionSelector: '^1.0.0', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:1.0.0', '@foo/bar')).toStrictEqual({ versionSelector: '1.0.0', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:latest', '@foo/bar')).toStrictEqual({ versionSelector: 'latest', jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
  })

  test('succeeds on jsr specifiers that only specify scope and name (jsr:@<scope>/<name>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar' } as JsrSpec)
  })

  test('succeeds on jsr specifiers that specify scopes, names, and versions/ranges/tags (jsr:@<scope>/<name>@<version_selector>)', () => {
    expect(parseJsrSpecifier('jsr:@foo/bar@^1.0.0')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', versionSelector: '^1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@1.0.0')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', versionSelector: '1.0.0' } as JsrSpec)
    expect(parseJsrSpecifier('jsr:@foo/bar@latest')).toStrictEqual({ jsrPkgName: '@foo/bar', npmPkgName: '@jsr/foo__bar', versionSelector: 'latest' } as JsrSpec)
  })

  test('errors on jsr specifiers that contain names without scopes', () => {
    expect(() => parseJsrSpecifier('jsr:foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_MISSING_JSR_PACKAGE_SCOPE',
    }))
  })

  test('errors on jsr specifiers that contain scopes without names', () => {
    expect(() => parseJsrSpecifier('jsr:@foo@^1.0.0')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
    expect(() => parseJsrSpecifier('jsr:@foo')).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_JSR_PACKAGE_NAME',
    }))
  })
})
