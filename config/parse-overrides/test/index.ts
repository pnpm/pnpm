import { parseOverrides } from '@pnpm/parse-overrides'

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
