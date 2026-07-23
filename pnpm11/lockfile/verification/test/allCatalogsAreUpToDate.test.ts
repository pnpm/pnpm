import { expect, test } from '@jest/globals'
import { allCatalogsAreUpToDate } from '@pnpm/lockfile.verification'

test('allCatalogsAreUpToDate() accepts an equivalent Git catalog specifier', () => {
  expect(allCatalogsAreUpToDate(
    { default: { 'is-positive': 'github:kevva/is-positive#97edff6' } },
    {
      default: {
        'is-positive': {
          specifier: 'git+https://github.com/kevva/is-positive.git#97edff6',
          version: 'git+https://github.com/kevva/is-positive.git#97edff6',
        },
      },
    }
  )).toBe(true)
})

test('allCatalogsAreUpToDate() reports drift for a different Git catalog specifier', () => {
  expect(allCatalogsAreUpToDate(
    { default: { 'is-positive': 'github:kevva/different#97edff6' } },
    {
      default: {
        'is-positive': {
          specifier: 'git+https://github.com/kevva/is-positive.git#97edff6',
          version: 'git+https://github.com/kevva/is-positive.git#97edff6',
        },
      },
    }
  )).toBe(false)
})

test('allCatalogsAreUpToDate() still matches plain specifiers', () => {
  expect(allCatalogsAreUpToDate(
    { default: { react: '^18.2.0' } },
    { default: { react: { specifier: '^18.2.0', version: '18.2.0' } } }
  )).toBe(true)
})
