import { shouldSyncInjectedDepsAfterScripts } from '../src/shouldSyncInjectedDepsAfterScripts'

describe(shouldSyncInjectedDepsAfterScripts, () => {
  test('undefined → false', () => {
    expect(shouldSyncInjectedDepsAfterScripts('build', undefined)).toBe(false)
  })

  test('false → false', () => {
    expect(shouldSyncInjectedDepsAfterScripts('build', false)).toBe(false)
  })

  test('true → true', () => {
    expect(shouldSyncInjectedDepsAfterScripts('build', true)).toBe(true)
  })

  test('unmatched → false', () => {
    expect(shouldSyncInjectedDepsAfterScripts('build', ['compile'])).toBe(false)
  })

  test('matched → true', () => {
    expect(shouldSyncInjectedDepsAfterScripts('build', ['build', 'compile'])).toBe(true)
  })
})
