import { DISABLE_DEPS_CHECK_ENV, shouldRunCheck } from '../src/shouldRunCheck'

test('should return true if no special env is defined', () => {
  expect(shouldRunCheck({})).toBe(true)
})

test('should return false if skip env is defined', () => {
  expect(shouldRunCheck({ ...DISABLE_DEPS_CHECK_ENV })).toBe(false)
})

describe('should return false if npm_lifecycle_event is an install hook', () => {
  test.each([
    'preinstall',
    'install',
    'postinstall',
    'preuninstall',
    'uninstall',
    'postuninstall',
  ])('%s', value => {
    expect(shouldRunCheck({ npm_lifecycle_event: value })).toBe(false)
  })
})

describe('should return true if npm_lifecycle_event is not an install hook', () => {
  test.each([
    'test',
    'build',
    'anything',
  ])('%s', value => {
    expect(shouldRunCheck({ npm_lifecycle_event: value })).toBe(true)
  })
})
