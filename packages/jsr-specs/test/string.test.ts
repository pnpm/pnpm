import {
  jsrToNpmPackageName,
  jsrToNpmSpecifier,
} from '../src/string'

test('jsrToNpmPackageName', () => {
  expect(jsrToNpmPackageName({ scope: 'foo', name: 'bar' })).toBe('@jsr/foo__bar')
})

test('jsrToNpmSpecifier', () => {
  expect(jsrToNpmSpecifier({ scope: 'foo', name: 'bar' })).toBe('npm:@jsr/foo__bar')
  expect(jsrToNpmSpecifier({ scope: 'foo', name: 'bar', pref: '^1.0.0' })).toBe('npm:@jsr/foo__bar@^1.0.0')
})
