import { parseOverrides } from '@pnpm/parse-overrides'

test.each([
  [
    { foo: '1' },
    [{ selector: 'foo', newPref: '1', targetPkg: { name: 'foo' } }],
  ],
  [
    { 'foo@2': '1' },
    [{ selector: 'foo@2', newPref: '1', targetPkg: { name: 'foo', pref: '2' } }],
  ],
  [
    {
      'foo@>2': '1',
      'foo@3 || >=2': '1',
    },
    [
      { selector: 'foo@>2', newPref: '1', targetPkg: { name: 'foo', pref: '>2' } },
      { selector: 'foo@3 || >=2', newPref: '1', targetPkg: { name: 'foo', pref: '3 || >=2' } },
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
      { selector: 'bar>foo', newPref: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo' } },
      { selector: 'bar@1>foo', newPref: '2', parentPkg: { name: 'bar', pref: '1' }, targetPkg: { name: 'foo' } },
      { selector: 'bar>foo@1', newPref: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo', pref: '1' } },
      { selector: 'bar@1>foo@1', newPref: '2', parentPkg: { name: 'bar', pref: '1' }, targetPkg: { name: 'foo', pref: '1' } },
    ],
  ],
  [
    {
      'foo@>2>bar@>2': '1',
      'foo@3 || >=2>bar@3 || >=2': '1',
    },
    [
      { selector: 'foo@>2>bar@>2', newPref: '1', parentPkg: { name: 'foo', pref: '>2' }, targetPkg: { name: 'bar', pref: '>2' } },
      { selector: 'foo@3 || >=2>bar@3 || >=2', newPref: '1', parentPkg: { name: 'foo', pref: '3 || >=2' }, targetPkg: { name: 'bar', pref: '3 || >=2' } },
    ],
  ],
])('parseOverrides()', (overrides, expectedResult) => {
  expect(parseOverrides(overrides, {})).toEqual(expectedResult)
})

test('parseOverrides() throws an exception on invalid selector', () => {
  expect(() => parseOverrides({ '%': '2' }, {})).toThrow('Cannot parse the "%" selector')
  expect(() => parseOverrides({ 'foo > bar': '2' }, {})).toThrow('Cannot parse the "foo > bar" selector')
})
