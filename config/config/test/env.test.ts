import path from 'path'
import url from 'url'
import { type ConfigPair, type GetSchema, type Schema, parseEnvVars } from '../src/env.js'

function assertSchemaKey (key: string): void {
  const strictlyKebabCase = key
    .split('-')
    .every(segment => /^[a-z0-9]+$/.test(segment))
  if (!strictlyKebabCase) {
    throw new Error(`Key ${key} is not strictly kebab-case`)
  }
}

const schemaGetter = (getSchema: GetSchema): GetSchema => key => {
  assertSchemaKey(key)
  return getSchema(key)
}

const schemaDict = (dict: Record<string, Schema | undefined>): GetSchema => schemaGetter(key => dict[key])

const alwaysSchema = (schema: Schema): GetSchema => schemaGetter(() => schema)

const pairsToObject = <Value> (pairs: Iterable<ConfigPair<Value>>): Record<string, Value> =>
  Object.fromEntries(Array.from(pairs).map(({ key, value }) => [key, value]))

test('parseEnvVars works with strings', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(String), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_abc_def_ghi: 'value of abcDefGhi',
    pnpm_config_foo: 'value of foo',
    pnpm_config_bar: 'value of bar',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    abcDefGhi: 'value of abcDefGhi',
    foo: 'value of foo',
    bar: 'value of bar',
  })
})

test('parseEnvVars works with numbers', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(Number), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_abc_def_ghi: '123',
    pnpm_config_foo: '456',
    pnpm_config_bar: '789',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    abcDefGhi: 123,
    foo: 456,
    bar: 789,
  })
})

test('parseEnvVars works with booleans', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(Boolean), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_foo: 'false',
    pnpm_config_bar: 'true',
    pnpm_config_baz: 'not a boolean',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    foo: false,
    bar: true,
    baz: undefined,
  })
})

test('parseEnvVars works with arrays', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(Array), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_foo: '[0, 1, 2]',
    pnpm_config_bar: '["a", "b"]',
    pnpm_config_baz: 'not an array',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    foo: [0, 1, 2],
    bar: ['a', 'b'],
    baz: undefined,
  })
})

test('parseEnvVars works with paths', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(path), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_foo: 'abc/def/ghi',
    pnpm_config_bar: '~/abc/def/ghi',
    pnpm_config_baz: '~\\abc\\def\\ghi',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    foo: 'abc/def/ghi',
    bar: path.join('/home/fake-user', 'abc/def/ghi'),
    baz: path.join('/home/fake-user', 'abc\\def\\ghi'),
  })

  expect(pairsToObject(parseEnvVars(alwaysSchema(path), {
    pnpm_config_foo: 'abc/def/ghi',
    pnpm_config_bar: '~/abc/def/ghi',
    pnpm_config_baz: '~\\abc\\def\\ghi',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    foo: 'abc/def/ghi',
    bar: '~/abc/def/ghi',
    baz: '~\\abc\\def\\ghi',
  })
})

test('parseEnvVars works with URLs', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(url), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_foo: 'https://registry.npmjs.com',
    pnpm_config_bar: 'http://example.org',
    pnpm_config_baz: 'file:///path/to/some/local/file',
    pnpm_config_undefined_somehow: undefined,
  }))).toStrictEqual({
    foo: 'https://registry.npmjs.com/',
    bar: 'http://example.org/',
    baz: 'file:///path/to/some/local/file',
  })
})

test('parseEnvVars works with literals', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(['foo', 'bar', true, null]), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_a: 'foo',
    pnpm_config_b: 'bar',
    pnpm_config_c: 'baz',
    pnpm_config_d: 'false',
    pnpm_config_e: 'true',
    pnpm_config_f: 'null',
  }))).toStrictEqual({
    a: 'foo',
    b: 'bar',
    c: undefined,
    d: undefined,
    e: true,
    f: null,
  })
})

test('parseEnvVars works with union', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(['foo', 'bar', Number, Array]), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_a: 'foo',
    pnpm_config_b: 'bar',
    pnpm_config_c: 'baz',
    pnpm_config_d: '123',
    pnpm_config_e: '456',
    pnpm_config_f: '[0, 1, "abc"]',
  }))).toStrictEqual({
    a: 'foo',
    b: 'bar',
    c: undefined,
    d: 123,
    e: 456,
    f: [0, 1, 'abc'],
  })
})

test('parseEnvVars prioritizes parsing non-strings', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema([String, Number, Array, Boolean]), {
    HOME: '/home/fake-user',
    PATH: '/bin:/usr/bin:/usr/local/bin:/home/fake-user/.bin:/home/fake-user/share/local/bin',
    pnpm_config_a: 'foo',
    pnpm_config_b: 'bar',
    pnpm_config_c: 'baz',
    pnpm_config_d: '123',
    pnpm_config_e: '456',
    pnpm_config_f: '[0, 1, "abc"]',
  }))).toStrictEqual({
    a: 'foo',
    b: 'bar',
    c: 'baz',
    d: 123,
    e: 456,
    f: [0, 1, 'abc'],
  })
})

test('parseEnvVars skips undefined schema', () => {
  expect(pairsToObject(parseEnvVars(schemaDict({
    foo: String,
    bar: String,
  }), {
    pnpm_config_foo: 'from foo',
    pnpm_config_bar: 'from bar',
    pnpm_config_baz: 'from baz',
  }))).toStrictEqual({
    foo: 'from foo',
    bar: 'from bar',
  })
})

test('parseEnvVars skips npm_config_*', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(String), {
    npm_config_abc_def_ghi: 'value of abcDefGhi',
    npm_config_foo: 'value of foo',
    npm_config_bar: 'value of bar',
  }))).toStrictEqual({})
})

test('parseEnvVars only reads lower snake case keys', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(String), {
    PNPM_CONFIG_UPPER_SNAKE_CASE_KEY: 'whole key in upper snake case',
    pnpmConfigCamelCaseKey: 'whole key in snake case',
    'pnpm-config-kebab-case': 'whole key in kebab case',
    pnpm_config_UPPER_SNAKE_CASE_SUFFIX: 'suffix in upper snake case',
    pnpm_config_camelCaseSuffix: 'suffix in camel case',
    'pnpm_config_kebab-case-suffix': 'suffix in kebab case',
    pnpm_config_lower_snake_case_key: 'whole key in lower snake case',
  }))).toStrictEqual({
    lowerSnakeCaseKey: 'whole key in lower snake case',
  })
})

test('parseEnvVars skips keys that contain multiple consecutive underscore characters', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(String), {
    pnpm_config_foo__bar: 'foo bar',
    pnpm_config_abc_def__ghi___jkl: 'abc def ghi jkl',
  }))).toStrictEqual({})
})

test('parseEnvVars skips keys that end with underscore character', () => {
  expect(pairsToObject(parseEnvVars(alwaysSchema(String), {
    pnpm_config_foo_bar_: 'foo bar',
  }))).toStrictEqual({})
})
