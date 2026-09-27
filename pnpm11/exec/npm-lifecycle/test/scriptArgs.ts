import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { appendScriptArgs, quoteForCmd } from '../src/scriptArgs.js'

test.each([
  ['plain', 'plain'],
  ['', '""'],
  ['C:\\Program Files\\tool', '^"C:\\Program^ Files\\tool^"'],
  ['C:\\Program Files\\tool\\', '^"C:\\Program^ Files\\tool\\\\^"'],
  ['C:\\dir\\', 'C:\\dir\\'],
  ['%PATH%', '^%PATH^%'],
  ['a"b', '^"a\\^"b^"'],
  ['a\\"b', '^"a\\\\\\^"b^"'],
  ['tab\there', '^"tab\there^"'],
  ['^&|<>()!', '^^^&^|^<^>^(^)^!'],
  ['line\nbreak', 'line\\nbreak'],
])('quoteForCmd(%j)', (arg, expected) => {
  expect(quoteForCmd(arg, false)).toBe(expected)
})

test('quoteForCmd() escapes twice for a batch file', () => {
  expect(quoteForCmd('%PATH%', true)).toBe('^^^%PATH^^^%')
  expect(quoteForCmd('a b', true)).toBe('^^^"a^^^ b^^^"')
})

test('appendScriptArgs() quotes for sh outside of cmd', () => {
  const searchPath = () => {
    throw new Error('searchPath is only read for cmd')
  }
  expect(appendScriptArgs('node x.js', ['a b', '%PATH%'], { platform: 'linux', searchPath })).toBe("node x.js 'a b' %PATH%")
  expect(appendScriptArgs('node x.js', ['a b'], { platform: 'win32', shellEmulator: true, searchPath })).toBe("node x.js 'a b'")
  expect(appendScriptArgs('node x.js', [], { platform: 'win32', searchPath })).toBe('node x.js')
})

test('appendScriptArgs() escapes twice when the script starts with a batch file', () => {
  const binDir = temporaryDirectory()
  const batchFile = path.join(binDir, 'tool.cmd')
  fs.writeFileSync(batchFile, '', { mode: 0o755 })
  const opts = { platform: 'win32' as const, searchPath: () => binDir }

  expect(appendScriptArgs('tool.cmd --flag', ['%PATH%'], opts)).toBe('tool.cmd --flag ^^^%PATH^^^%')
  expect(appendScriptArgs(`"${batchFile}" --flag`, ['%PATH%'], opts)).toBe(`"${batchFile}" --flag ^^^%PATH^^^%`)
  expect(appendScriptArgs('node tool.cmd', ['%PATH%'], opts)).toBe('node tool.cmd ^%PATH^%')
})
