import { expect, test } from '@jest/globals'

import { selectShell } from '../src/selectShell.js'

test('uses sh -c by default on POSIX', () => {
  expect(selectShell(undefined, 'linux', undefined)).toEqual({ sh: 'sh', shFlag: '-c', windowsVerbatimArguments: false })
})

test('uses ComSpec with /d /s /c by default on Windows', () => {
  expect(selectShell(undefined, 'win32', 'C:\\Windows\\system32\\cmd.exe')).toEqual({
    sh: 'C:\\Windows\\system32\\cmd.exe',
    shFlag: '/d /s /c',
    windowsVerbatimArguments: true,
  })
})

test.each([
  'C:\\Windows\\System32\\cmd.exe',
  'C:/Windows/System32/CMD.EXE',
  'cmd.exe',
  'cmd',
  'Cmd',
])('passes /d /s /c to a cmd.exe scriptShell on Windows (%s)', (scriptShell) => {
  expect(selectShell(scriptShell, 'win32', undefined)).toEqual({ sh: scriptShell, shFlag: '/d /s /c', windowsVerbatimArguments: true })
})

test('passes -c to a non-cmd scriptShell on Windows', () => {
  const scriptShell = 'C:\\Program Files\\Git\\bin\\bash.exe'
  expect(selectShell(scriptShell, 'win32', undefined)).toEqual({ sh: scriptShell, shFlag: '-c', windowsVerbatimArguments: false })
})

test('passes -c to a scriptShell named cmd on POSIX', () => {
  expect(selectShell('/usr/local/bin/cmd', 'linux', undefined)).toEqual({ sh: '/usr/local/bin/cmd', shFlag: '-c', windowsVerbatimArguments: false })
})
