import { expect, test } from '@jest/globals'
import { load } from 'js-yaml'

import { dumpYaml } from '../src/yamlDump.js'

// The expected output is what js-yaml 4 wrote, which existing lockfiles are formatted with.
test.each([
  ['react', 'react'],
  ['', "''"],
  ['=4.2.2', "'=4.2.2'"],
  ['^1.0.0', '^1.0.0'],
  ['>=10', "'>=10'"],
  ['9.0', "'9.0'"],
  ['1', "'1'"],
  ['0x1F', "'0x1F'"],
  ['1e5', "'1e5'"],
  ['.inf', "'.inf'"],
  ['+.5', "'+.5'"],
  ['-.5', "'-.5'"],
  ['1_000', '1_000'],
  ['1e999', "'1e999'"],
  ['9'.repeat(400), `'${'9'.repeat(400)}'`],
  ['yes', 'yes'],
  ['on', 'on'],
  ['null', "'null'"],
  ['~', "'~'"],
  ['true', "'true'"],
  ['2020-01-02', "'2020-01-02'"],
  ['<<', "'<<'"],
  ['@scope/pkg@1.0.0', "'@scope/pkg@1.0.0'"],
  ['a: b', "'a: b'"],
  ['a:b', 'a:b'],
  ['a #b', "'a #b'"],
  ['a#b', 'a#b'],
  ['- x', "'- x'"],
  ['x ', "'x '"],
  ['tab\there', '"tab\\there"'],
  ['é😀', 'é😀'],
  ['\u0085', '"\\N"'],
  ['first\nsecond', '|-\n  first\n  second'],
  [' indented\n', '|2\n   indented'],
  ['kept\n\n', '|+\n  kept\n'],
  ['link:../a', 'link:../a'],
])('dumpYaml() writes the scalar %j as %s', (value, expected) => {
  const serialized = dumpYaml({ key: value })
  expect(serialized).toBe(`key: ${expected}\n`)
  expect(load(serialized)).toStrictEqual({ key: value })
})

test('dumpYaml() writes collections from the flow level in flow style', () => {
  expect(dumpYaml({ cpu: ['x64', 'a,b'], engines: { node: '>=18', npm: 'first\nsecond' } }, { flowLevel: 1 }))
    .toBe("cpu: [x64, 'a,b']\nengines: {node: '>=18', npm: \"first\\nsecond\"}\n")
})

test('dumpYaml() writes a hole in a flow sequence as null', () => {
  expect(dumpYaml({ cpu: Array(1) }, { flowLevel: 1 })).toBe('cpu: [null]\n')
})

test('dumpYaml() writes non-string values, skips undefined mapping values, and writes undefined sequence items as null', () => {
  expect(dumpYaml({ list: [1, -0, 1.5e-7, true, null, undefined, [], {}], skipped: undefined, nested: [{ a: 1, b: [2] }] })).toBe([
    'list:',
    '  - 1',
    '  - -0.0',
    '  - 1.5e-7',
    '  - true',
    '  - null',
    '  - null',
    '  - []',
    '  - {}',
    'nested:',
    '  - a: 1',
    '    b:',
    '      - 2',
    '',
  ].join('\n'))
})
