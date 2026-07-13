import { expect, test } from '@jest/globals'
import { parseOverrides } from '@pnpm/config.parse-overrides'

test.each([
  [
    { foo: '1' },
    [{ selector: 'foo', newBareSpecifier: '1', targetPkg: { name: 'foo' } }],
  ],
  [
    { 'foo@2': '1' },
    [{ selector: 'foo@2', newBareSpecifier: '1', targetPkg: { name: 'foo', bareSpecifier: '2' } }],
  ],
  [
    {
      'foo@>2': '1',
      'foo@3 || >=2': '1',
    },
    [
      { selector: 'foo@>2', newBareSpecifier: '1', targetPkg: { name: 'foo', bareSpecifier: '>2' } },
      { selector: 'foo@3 || >=2', newBareSpecifier: '1', targetPkg: { name: 'foo', bareSpecifier: '3 || >=2' } },
    ],
  ],
  [
    {
      'bar>foo': '2',
      'bar@1>foo': '2',
      'bar>foo@1': '2',
      'bar@1>foo@1': '2',
    },
    [
      { selector: 'bar>foo', newBareSpecifier: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo' } },
      { selector: 'bar@1>foo', newBareSpecifier: '2', parentPkg: { name: 'bar', bareSpecifier: '1' }, targetPkg: { name: 'foo' } },
      { selector: 'bar>foo@1', newBareSpecifier: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo', bareSpecifier: '1' } },
      { selector: 'bar@1>foo@1', newBareSpecifier: '2', parentPkg: { name: 'bar', bareSpecifier: '1' }, targetPkg: { name: 'foo', bareSpecifier: '1' } },
    ],
  ],
  [
    {
      'foo@>2>bar@>2': '1',
      'foo@3 || >=2>bar@3 || >=2': '1',
    },
    [
      { selector: 'foo@>2>bar@>2', newBareSpecifier: '1', parentPkg: { name: 'foo', bareSpecifier: '>2' }, targetPkg: { name: 'bar', bareSpecifier: '>2' } },
      { selector: 'foo@3 || >=2>bar@3 || >=2', newBareSpecifier: '1', parentPkg: { name: 'foo', bareSpecifier: '3 || >=2' }, targetPkg: { name: 'bar', bareSpecifier: '3 || >=2' } },
    ],
  ],
])('parseOverrides()', (overrides, expectedResult) => {
  expect(parseOverrides(overrides, {})).toEqual(expectedResult)
})

test('parseOverrides() throws an exception on invalid selector', () => {
  expect(() => parseOverrides({ '%': '2' }, {})).toThrow('Cannot parse the "%" selector')
  expect(() => parseOverrides({ 'foo > bar': '2' }, {})).toThrow('Cannot parse the "foo > bar" selector')
})

test.each([
  ['foo@', '1.2.3'],
  ['@scope/foo@', '2.0.0-beta.1'],
])('parseOverrides() parses the "%s" convergence override', (selector, version) => {
  expect(parseOverrides({ [selector]: version }, {})).toEqual([
    {
      selector,
      newBareSpecifier: version,
      targetPkg: { name: selector.slice(0, -1), bareSpecifier: '' },
      converge: true,
    },
  ])
})

test('parseOverrides() resolves a catalog value of a convergence override', () => {
  expect(parseOverrides({ 'foo@': 'catalog:' }, { default: { foo: '1.2.3' } })).toEqual([
    {
      selector: 'foo@',
      newBareSpecifier: '1.2.3',
      targetPkg: { name: 'foo', bareSpecifier: '' },
      converge: true,
    },
  ])
})

test.each([
  ['^1.2.3'],
  ['latest'],
  ['-'],
  ['link:../foo'],
  ['npm:bar@1.2.3'],
])('parseOverrides() throws when the value of a convergence override is "%s"', (value) => {
  expect(() => parseOverrides({ 'foo@': value }, {})).toThrow(
    `The value of the convergence override "foo@" must be an exact version, but got "${value}"`
  )
})

test('parseOverrides() throws when an empty range is used in a parent>child selector', () => {
  expect(() => parseOverrides({ 'bar>foo@': '1.2.3' }, {})).toThrow(
    'Cannot use an empty range in the "bar>foo@" selector'
  )
})
