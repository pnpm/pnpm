import parseOverrides from '@pnpm/parse-overrides'

test.each([
  [
    { foo: '1' },
    [{ newPref: '1', targetPkg: { name: 'foo' } }],
  ],
  [
    { 'foo@2': '1' },
    [{ newPref: '1', targetPkg: { name: 'foo', pref: '2' } }],
  ],
  [
    {
      'bar>foo': '2',
      'bar@1>foo': '2',
      'bar>foo@1': '2',
      'bar@1>foo@1': '2',
    },
    [
      { newPref: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo' } },
      { newPref: '2', parentPkg: { name: 'bar', pref: '1' }, targetPkg: { name: 'foo' } },
      { newPref: '2', parentPkg: { name: 'bar' }, targetPkg: { name: 'foo', pref: '1' } },
      { newPref: '2', parentPkg: { name: 'bar', pref: '1' }, targetPkg: { name: 'foo', pref: '1' } },
    ],
  ],
  [
    { 'foo@2': '1' },
    [{ newPref: '1', targetPkg: { name: 'foo', pref: '2' } }],
  ],
])('parseOverrides()', (overrides, expectedResult) => {
  expect(parseOverrides(overrides)).toEqual(expectedResult)
})

test('parseOverrides() throws an exception on invalid selector', () => {
  expect(() => parseOverrides({ '%': '2' })).toThrow('Cannot parse the "%" selector in the overrides')
})
