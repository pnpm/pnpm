import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { appendScriptArgs, quoteForCmd, showScriptWithArgs } from '../src/scriptArgs.js'

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
  const opts = { wd: process.cwd(), searchPath }
  expect(appendScriptArgs('node x.js', ['a b', '%PATH%'], { ...opts, platform: 'linux' })).toBe("node x.js 'a b' %PATH%")
  expect(appendScriptArgs('node x.js', ['a b'], { ...opts, platform: 'win32', shellEmulator: true })).toBe("node x.js 'a b'")
  expect(appendScriptArgs('node x.js', [], { ...opts, platform: 'win32' })).toBe('node x.js')
})

test.each([
  ['tool.cmd --flag', true],
  ['tool.cmd\t--flag', true],
  ['"<bin>/tool.cmd" --flag', true],
  ['bin/tool.cmd --flag', true],
  ['echo ready && tool.cmd', true],
  ['tool.cmd 2>&1', true],
  ['tool.cmd <&0', true],
  ['echo ^>& tool.cmd', true],
  ['echo ^<& tool.cmd', true],
  ['node x.js ^& tool.cmd', false],
  ['tool.cmd && node x.js', false],
  ['node tool.cmd', false],
])('appendScriptArgs() escapes twice only when %j ends with a batch file', (script, doubleEscape) => {
  const wd = temporaryDirectory()
  const binDir = path.join(wd, 'bin')
  fs.mkdirSync(binDir)
  fs.writeFileSync(path.join(binDir, 'tool.cmd'), '', { mode: 0o755 })
  script = script.replace('<bin>', binDir)

  expect(appendScriptArgs(script, ['%PATH%'], { platform: 'win32', wd, searchPath: () => binDir }))
    .toBe(`${script} ${doubleEscape ? '^^^%PATH^^^%' : '^%PATH^%'}`)
})

test('appendScriptArgs() finds a batch file in the directory the script runs in', () => {
  const wd = temporaryDirectory()
  fs.writeFileSync(path.join(wd, 'tool.cmd'), '', { mode: 0o755 })

  expect(appendScriptArgs('tool.cmd', ['%PATH%'], { platform: 'win32', wd, searchPath: () => '' }))
    .toBe('tool.cmd ^^^%PATH^^^%')
})

test('showScriptWithArgs() quotes the POSIX way', () => {
  expect(showScriptWithArgs('node x.js', ['a b', '%PATH%', 'C:\\dir\\'])).toBe("node x.js 'a b' %PATH% 'C:\\dir\\'")
  expect(showScriptWithArgs('node x.js', [])).toBe('node x.js')
})
