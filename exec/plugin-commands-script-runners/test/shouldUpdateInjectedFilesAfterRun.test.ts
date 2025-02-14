import { shouldUpdateInjectedFilesAfterRun } from '../src/shouldUpdateInjectedFilesAfterRun'

describe(shouldUpdateInjectedFilesAfterRun, () => {
  test('undefined → false', () => {
    expect(shouldUpdateInjectedFilesAfterRun('build', undefined)).toBe(false)
  })

  test('false → false', () => {
    expect(shouldUpdateInjectedFilesAfterRun('build', false)).toBe(false)
  })

  test('true → true', () => {
    expect(shouldUpdateInjectedFilesAfterRun('build', true)).toBe(true)
  })

  test('unmatched → false', () => {
    expect(shouldUpdateInjectedFilesAfterRun('build', ['compile'])).toBe(false)
  })

  test('matched → true', () => {
    expect(shouldUpdateInjectedFilesAfterRun('build', ['build', 'compile'])).toBe(true)
  })
})
