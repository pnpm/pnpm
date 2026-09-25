import { expect, test } from '@jest/globals'

import { scriptBody, selectShell } from '../src/selectShell.js'

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

test('a Bourne shell returns the interrupted child status', () => {
  const shell = selectShell(undefined, 'linux', undefined)
  const body = scriptBody(shell, 'node dev.js')
  expect(body.startsWith('trap ')).toBe(true)
  expect(body.endsWith('node dev.js')).toBe(true)
  expect(body).toContain('-eq 130')
  expect(scriptBody(selectShell('/usr/bin/bash.exe', 'win32', undefined), 'node dev.js').startsWith('trap ')).toBe(true)
})

test('a non-Bourne shell runs the command unchanged', () => {
  expect(scriptBody(selectShell('/usr/bin/fish', 'linux', undefined), 'node dev.js')).toBe('node dev.js')
  expect(scriptBody(selectShell('cmd.exe', 'win32', undefined), 'node dev.js')).toBe('node dev.js')
})
