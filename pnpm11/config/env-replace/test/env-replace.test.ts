import { describe, expect, test } from '@jest/globals'
import { envReplace, envReplaceLossy } from '@pnpm/config.env-replace'

const ENV = {
  foo: 'foo_value',
  bar: 'bar_value',
  zoo: '',
}

test.each([
  ['-${foo}-${bar}', '-foo_value-bar_value'],
  ['\\${foo}', '${foo}'],
  ['\\${zoo}', '${zoo}'],
  ['\\\\${foo}', '\\foo_value'],
  ['-${foo-fallback-value}-${bar:-fallback-value}', '-foo_value-bar_value'],
  ['-${qar-fallback-value}-${zoo-fallback-value}', '-fallback-value-'],
  ['-${qar-fallback-value}-${zoo:-fallback-for-empty-value}', '-fallback-value-fallback-for-empty-value'],
])('success %s => %s', (settingValue, expected) => {
  const actual = envReplace(settingValue, ENV)
  expect(actual).toEqual(expected)
})

test('fail when the env variable is not found', () => {
  expect(() => envReplace('${baz}', ENV)).toThrow('Failed to replace env in config: ${baz}')
  expect(() => envReplace('${foo-}', ENV)).toThrow('Failed to replace env in config: ${foo-}')
  expect(() => envReplace('${foo:-}', ENV)).toThrow('Failed to replace env in config: ${foo:-}')
})

test('treat present-but-undefined env value as unset for fallbacks', () => {
  // `NodeJS.ProcessEnv` is `Record<string, string | undefined>`. Callers that
  // construct the env object directly may represent an unset variable as
  // `{ KEY: undefined }`. `${KEY-default}` must use the fallback in that case.
  const ENV_WITH_UNDEFINED = { ...ENV, qux: undefined }
  expect(envReplace('${qux-fallback}', ENV_WITH_UNDEFINED)).toBe('fallback')
  expect(envReplace('${qux:-fallback}', ENV_WITH_UNDEFINED)).toBe('fallback')
  expect(() => envReplace('${qux}', ENV_WITH_UNDEFINED))
    .toThrow('Failed to replace env in config: ${qux}')
})

describe('envReplaceLossy', () => {
  test('substitutes unresolved placeholders with empty string and records them', () => {
    const { value, unresolved } = envReplaceLossy('${baz}', ENV)
    expect(value).toBe('')
    expect(unresolved).toEqual(['${baz}'])
  })

  test('preserves resolvable placeholders and default fallbacks alongside unresolved ones', () => {
    // Mixed value: one resolvable, one unresolved bare, one with a `-default` fallback.
    // Only the bare unresolved one becomes ''; the others still expand normally.
    const { value, unresolved } = envReplaceLossy(
      '${foo}-${baz}-${qux-fallback}',
      ENV
    )
    expect(value).toBe('foo_value--fallback')
    expect(unresolved).toEqual(['${baz}'])
  })

  test('records every unresolved placeholder occurrence in source order', () => {
    const { value, unresolved } = envReplaceLossy('${a}-${b}-${a}', {})
    expect(value).toBe('--')
    expect(unresolved).toEqual(['${a}', '${b}', '${a}'])
  })

  test('returns the value unchanged and empty unresolved when nothing fails', () => {
    const { value, unresolved } = envReplaceLossy('-${foo}-${bar}-', ENV)
    expect(value).toBe('-foo_value-bar_value-')
    expect(unresolved).toEqual([])
  })

  test('respects backslash escapes the same way envReplace does', () => {
    // Odd backslash count escapes the placeholder; lookup never runs.
    expect(envReplaceLossy('\\${baz}', ENV)).toEqual({ value: '${baz}', unresolved: [] })
    // Even count: half collapses to a literal `\`, the placeholder expands.
    expect(envReplaceLossy('\\\\${foo}', ENV)).toEqual({ value: '\\foo_value', unresolved: [] })
  })
})
