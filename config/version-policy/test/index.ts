import { expect, test } from '@jest/globals'
import {
  createPackageVersionPolicy,
  createPackageVersionPolicyOrThrow,
  getPublishedByPolicy,
} from '@pnpm/config.version-policy'

test('createPackageVersionPolicy()', () => {
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
  }
  {
    const match = createPackageVersionPolicy(['is-*'])
    expect(match('is-odd')).toBe(true)
    expect(match('is-even')).toBe(true)
    expect(match('lodash')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['@babel/core@7.20.0'])
    expect(match('@babel/core')).toStrictEqual(['7.20.0'])
  }
  {
    const match = createPackageVersionPolicy(['@babel/core'])
    expect(match('@babel/core')).toBe(true)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2'])
    expect(match('is-odd')).toBe(false)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.2', 'lodash@4.17.21', 'is-*'])
    expect(match('axios')).toStrictEqual(['1.12.2'])
    expect(match('lodash')).toStrictEqual(['4.17.21'])
    expect(match('is-odd')).toBe(true)
  }
  {
    expect(() => createPackageVersionPolicy(['lodash@^4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['lodash@~4.17.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['react@>=18.0.0'])).toThrow(/Invalid versions union/)
    expect(() => createPackageVersionPolicy(['is-*@1.0.0'])).toThrow(/Name patterns are not allowed/)
  }
  {
    const match = createPackageVersionPolicy(['axios@1.12.0 || 1.12.1'])
    expect(match('axios')).toStrictEqual(['1.12.0', '1.12.1'])
  }
  {
    const match = createPackageVersionPolicy(['@scope/pkg@1.0.0 || 1.0.1'])
    expect(match('@scope/pkg')).toStrictEqual(['1.0.0', '1.0.1'])
  }
  {
    const match = createPackageVersionPolicy(['pkg@1.0.0||1.0.1  ||  1.0.2'])
    expect(match('pkg')).toStrictEqual(['1.0.0', '1.0.1', '1.0.2'])
  }
})

test('createPackageVersionPolicyOrThrow() rewraps parser errors with INVALID_<KEY>', () => {
  expect(() => createPackageVersionPolicyOrThrow(['lodash@^4.17.0'], 'minimumReleaseAgeExclude')).toThrow(
    expect.objectContaining({
      code: 'ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE',
      message: expect.stringContaining('Invalid value in minimumReleaseAgeExclude:'),
    })
  )
  expect(() => createPackageVersionPolicyOrThrow(['is-*@1.0.0'], 'trustPolicyExclude')).toThrow(
    expect.objectContaining({
      code: 'ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE',
    })
  )
})

test('createPackageVersionPolicyOrThrow() returns a working policy for valid input', () => {
  const policy = createPackageVersionPolicyOrThrow(['axios@1.12.2'], 'minimumReleaseAgeExclude')
  expect(policy('axios')).toStrictEqual(['1.12.2'])
  expect(policy('lodash')).toBe(false)
})

test('getPublishedByPolicy() returns undefined fields when no config is set', () => {
  expect(getPublishedByPolicy({})).toEqual({
    publishedBy: undefined,
    publishedByExclude: undefined,
  })
})

test('getPublishedByPolicy() derives publishedBy from minimumReleaseAge (minutes)', () => {
  const before = Date.now()
  const { publishedBy, publishedByExclude } = getPublishedByPolicy({ minimumReleaseAge: 24 * 60 })
  const after = Date.now()
  expect(publishedByExclude).toBeUndefined()
  expect(publishedBy).toBeInstanceOf(Date)
  // 24h ago, ± the wall-clock drift between sampling `before` and `after`.
  const expectedMin = before - 24 * 60 * 60 * 1000
  const expectedMax = after - 24 * 60 * 60 * 1000
  expect(publishedBy!.getTime()).toBeGreaterThanOrEqual(expectedMin)
  expect(publishedBy!.getTime()).toBeLessThanOrEqual(expectedMax)
})

test('getPublishedByPolicy() builds publishedByExclude policy when minimumReleaseAgeExclude is set', () => {
  const { publishedByExclude } = getPublishedByPolicy({
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm@9.1.0'],
  })
  expect(publishedByExclude!('pnpm')).toStrictEqual(['9.1.0'])
  expect(publishedByExclude!('axios')).toBe(false)
})

test('getPublishedByPolicy() rewraps invalid exclude patterns as ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE', () => {
  expect(() => getPublishedByPolicy({
    minimumReleaseAge: 24 * 60,
    minimumReleaseAgeExclude: ['pnpm@^9.0.0'],
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE' }))
})
