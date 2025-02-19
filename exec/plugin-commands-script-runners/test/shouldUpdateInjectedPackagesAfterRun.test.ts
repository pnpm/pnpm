import { shouldUpdateInjectedPackagesAfterRun } from '../src/shouldUpdateInjectedPackagesAfterRun'

describe(shouldUpdateInjectedPackagesAfterRun, () => {
  test('undefined → false', () => {
    expect(shouldUpdateInjectedPackagesAfterRun('build', undefined)).toBe(false)
  })

  test('false → false', () => {
    expect(shouldUpdateInjectedPackagesAfterRun('build', false)).toBe(false)
  })

  test('true → true', () => {
    expect(shouldUpdateInjectedPackagesAfterRun('build', true)).toBe(true)
  })

  test('unmatched → false', () => {
    expect(shouldUpdateInjectedPackagesAfterRun('build', ['compile'])).toBe(false)
  })

  test('matched → true', () => {
    expect(shouldUpdateInjectedPackagesAfterRun('build', ['build', 'compile'])).toBe(true)
  })
})
