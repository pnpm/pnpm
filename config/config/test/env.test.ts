import { type ConfigPair, type NpmConf, parseEnvVars } from '../src/env.js'

class MockedNpmConf implements NpmConf {
  data: Record<string, string | string[]>

  constructor () {
    this.data = {}
  }

  public get (key: string): string | string[] {
    return this.data[key]
  }

  public set (key: string, value: string | string[]) {
    this.data[key] = value
  }
}

const pairsToObject = <Value> (pairs: Iterable<ConfigPair<Value>>): Record<string, Value> =>
  Object.fromEntries(Array.from(pairs).map(({ key, value }) => [key, value]))

test('parseEnvVars works as expected', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
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

test('parseEnvVars treats hoist pattern as comma-separated list', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    pnpm_config_hoist_pattern: 'foo,bar,baz',
    pnpm_config_public_hoist_pattern: 'abc,def,ghi',
  }))).toStrictEqual({
    hoistPattern: ['foo', 'bar', 'baz'],
    publicHoistPattern: ['abc', 'def', 'ghi'],
  })
})

test('parseEnvVars treats hoist pattern as list of paragraphs', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    pnpm_config_hoist_pattern: 'foo,bar,baz\n\nabc,def\n\nhello',
  }))).toStrictEqual({
    hoistPattern: ['foo,bar,baz', 'abc,def', 'hello'],
  })
})

test('parseEnvVars skips npm_config_*', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    npm_config_abc_def_ghi: 'value of abcDefGhi',
    npm_config_foo: 'value of foo',
    npm_config_bar: 'value of bar',
  }))).toStrictEqual({})
})

test('parseEnvVars only reads lower snake case keys', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    PNPM_CONFIG_UPPER_SNAKE_CASE_KEY: 'whole key in upper snake case',
    pnpmConfigCamelCaseKey: 'whole key in snake case',
    'pnpm-config-kebab-case': 'whole key in kebab case',
    pnpm_config_UPPER_SNAKE_CASE_KEY: 'suffix in upper snake case',
    pnpm_config_camelCaseSuffix: 'suffix in camel case',
    'pnpm_config_kebab-case-suffix': 'suffix in kebab case',
    pnpm_config_lower_snake_case_key: 'whole key in lower snake case',
  }))).toStrictEqual({
    lowerSnakeCaseKey: 'whole key in lower snake case',
  })
})

test('parseEnvVars skips keys that contain multiple consecutive underscore characters', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    pnpm_config_foo__bar: 'foo bar',
    pnpm_config_abc_def__ghi___jkl: 'abc def ghi jkl',
  }))).toStrictEqual({})
})

test('parseEnvVars skips keys that end with underscore character', () => {
  expect(pairsToObject(parseEnvVars(new MockedNpmConf(), {
    pnpm_config_foo_bar_: 'foo bar',
  }))).toStrictEqual({})
})
