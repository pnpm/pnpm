import {
  createJsrPackageName,
  createNpmPackageName,
  createNpmPref,
} from '../src/string'

test('createJsrPackageName', () => {
  expect(createJsrPackageName({ scope: 'foo', name: 'bar' })).toBe('@foo/bar')
})

test('createNpmPackageName', () => {
  expect(createNpmPackageName({ scope: 'foo', name: 'bar' })).toBe('@jsr/foo__bar')
})

test('createNpmPref', () => {
  expect(createNpmPref({ scope: 'foo', name: 'bar' })).toBe('npm:@jsr/foo__bar')
  expect(createNpmPref({ scope: 'foo', name: 'bar', pref: '^1.0.0' })).toBe('npm:@jsr/foo__bar@^1.0.0')
})
