import { DISABLE_DEPS_CHECK_ENV, shouldRunCheck } from '../src/shouldRunCheck'

test('should return true if skip env is not defined and script name is not special', () => {
  expect(shouldRunCheck({}, 'start')).toBe(true)
})

test('should return false if skip env is defined', () => {
  expect(shouldRunCheck({ ...DISABLE_DEPS_CHECK_ENV }, 'start')).toBe(false)
})

test.each([
  'preinstall',
  'install',
  'postinstall',
  'preuninstall',
  'uninstall',
  'postuninstall',
])('should return false if script name is %p', scriptName => {
  expect(shouldRunCheck({}, scriptName)).toBe(false)
})
