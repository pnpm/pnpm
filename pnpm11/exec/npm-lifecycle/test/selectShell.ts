import { spawnSync } from 'node:child_process'

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

test('a Bourne shell runs the command behind the interrupt trap', () => {
  expect(scriptBody(selectShell(undefined, 'linux', undefined), 'node dev.js')).toMatch(/^trap .*node dev\.js$/)
  expect(scriptBody(selectShell('/usr/bin/bash.exe', 'win32', undefined), 'node dev.js')).toMatch(/^trap .*node dev\.js$/)
})

const skipOnWindows = process.platform === 'win32' ? test.skip : test

function runInSh (command: string) {
  const shell = selectShell(undefined, process.platform, undefined)
  return spawnSync(shell.sh, [shell.shFlag, scriptBody(shell, command)], { encoding: 'utf8' })
}

// A shell's own `kill` stands in for a terminal interrupt that the foreground
// command handled: the trap then sees that command's status, not 130.
skipOnWindows('a handled interrupt lets the rest of the script run', () => {
  const { stdout, status } = runInSh('kill -s INT $$; echo after; exit 3')
  expect(stdout).toBe('after\n')
  expect(status).toBe(3)
})

skipOnWindows('a command killed by the interrupt ends the script with SIGINT', () => {
  const { stdout, signal } = runInSh("sh -c 'kill -s INT $PPID; kill -s INT $$'; echo after")
  expect(stdout).toBe('')
  expect(signal).toBe('SIGINT')
})

skipOnWindows('a second interrupt ends the script with SIGINT', () => {
  const { stdout, signal } = runInSh('kill -s INT $$; kill -s INT $$; echo after')
  expect(stdout).toBe('')
  expect(signal).toBe('SIGINT')
})

test('a non-Bourne shell runs the command unchanged', () => {
  expect(scriptBody(selectShell('/usr/bin/fish', 'linux', undefined), 'node dev.js')).toBe('node dev.js')
  expect(scriptBody(selectShell('cmd.exe', 'win32', undefined), 'node dev.js')).toBe('node dev.js')
})
