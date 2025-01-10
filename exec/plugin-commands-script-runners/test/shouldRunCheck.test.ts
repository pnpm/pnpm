import { DISABLE_DEPS_CHECK_ENV, shouldRunCheck } from '../src/shouldRunCheck'

test('should return true if no special env is defined', () => {
  expect(shouldRunCheck({})).toBe(true)
})

test('should return false if skip env is defined', () => {
  expect(shouldRunCheck({ ...DISABLE_DEPS_CHECK_ENV })).toBe(false)
})

test('should return false if npm_lifecycle_even is defined', () => {
  expect(shouldRunCheck({ npm_lifecycle_event: 'anything' })).toBe(false)
})
