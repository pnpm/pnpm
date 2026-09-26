import { expect, test } from '@jest/globals'

import { commandParsedByCmd, selectShell, useShellEmulator } from '../src/selectShell.js'

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

const gitBash = 'C:\\Program Files\\Git\\bin\\bash.exe'

test('shellEmulator applies only when scriptShell is unset', () => {
  expect(useShellEmulator(true, undefined)).toBe(true)
  expect(useShellEmulator(true, '')).toBe(true)
  expect(useShellEmulator(true, gitBash)).toBe(false)
  expect(useShellEmulator(false, gitBash)).toBe(false)
  expect(useShellEmulator(false, undefined)).toBe(false)
})

test('extra arguments are JSON-quoted only when cmd will parse them', () => {
  expect(commandParsedByCmd(gitBash, 'win32', true)).toBe(false)
  expect(commandParsedByCmd(undefined, 'win32', false)).toBe(true)
  expect(commandParsedByCmd(undefined, 'win32', true)).toBe(false)
  expect(commandParsedByCmd('C:\\Windows\\System32\\cmd.exe', 'win32', false)).toBe(true)
  expect(commandParsedByCmd('C:\\Windows\\System32\\cmd.exe', 'win32', true)).toBe(true)
  expect(commandParsedByCmd(gitBash, 'linux', false)).toBe(false)
})
