import { expect, test } from '@jest/globals'
import { compareVersions } from '@pnpm/deps.compliance.license-scanner'

test('compareVersions() sorts versions that are not valid semver before valid ones', () => {
  for (const versions of [['2.0.0', '10.0.0', '1z'], ['1z', '10.0.0', '2.0.0'], ['10.0.0', '1z', '2.0.0']]) {
    expect([...versions].sort(compareVersions)).toStrictEqual(['1z', '2.0.0', '10.0.0'])
  }
  expect(compareVersions(undefined, '1.0.0')).toBeLessThan(0)
})
