import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeJsonFileSync } from 'write-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from './utils/index.js'

test('completion includes script shortcuts among built-in commands', () => {
  prepare({ scripts: { add: 'echo unused', 'build:watch': 'echo unused', hello: 'echo unused', '.hidden': 'echo unused', 'bad\nname': 'echo unused' } })
  fs.mkdirSync('src')
  const complete = (word: string, cwd?: string): string[] => {
    const line = `pn ${word}`
    const { status, stdout, stderr } = execPnpmSync(['completion-server', '--', 'pn', word], {
      cwd,
      env: { XDG_CONFIG_HOME: path.resolve('.config'), SHELL: 'bash', COMP_CWORD: '1', COMP_LINE: line, COMP_POINT: String(line.length) },
    })
    expect({ status, stderr: stderr.toString() }).toEqual({ status: 0, stderr: '' })
    return stdout.toString().trimEnd().split('\n')
  }
  expect(complete('he')).toEqual(expect.arrayContaining(['help', 'hello']))
  const names = complete('')
  expect(names).toEqual(expect.arrayContaining(['help', 'hello', 'build:watch']))
  expect(names.filter((name) => name === 'add')).toEqual(['add'])
  expect(names).not.toContain('.hidden')
  expect(names).not.toContain('bad')
  expect(names).not.toContain('name')
  expect(complete('build', 'src')).toContain('build:watch')
})

test.each(['bash', 'fish', 'pwsh', 'zsh'])('completion omits unsafe package and script names for %s', (shell) => {
  const unsafeNames = ['bad\nname', 'bad\rname', 'bad\tname', 'bad\u001B[31mname', 'bad\u007Fname', 'bad\u0085name', 'bad\u202Ename', 'bad\u2028name']
  if (shell === 'fish') unsafeNames.push('bad\\nname', 'bad\\ename', 'bad\\x1bname', 'bad\\u001Bname')
  prepare({ name: 'safe', scripts: Object.fromEntries(['safe', ...unsafeNames].map((name) => [name, 'echo unused'])) })
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  for (const [index, name] of unsafeNames.entries()) {
    const directory = `packages/pkg-${index}`
    fs.mkdirSync(directory, { recursive: true })
    writeJsonFileSync(`${directory}/package.json`, { name })
  }
  for (const option of ['--filter', '-F', 'run']) {
    const line = `pnpm ${option} `
    const { status, stdout, stderr } = execPnpmSync(['completion-server', '--', 'pnpm', option, ''], {
      env: { XDG_CONFIG_HOME: path.resolve('.config'), SHELL: shell, COMP_CWORD: '2', COMP_LINE: line, COMP_POINT: String(line.length) },
    })
    expect({ status, stderr: stderr.toString() }).toEqual({ status: 0, stderr: '' })
    expect(stdout.toString().split('\n').filter((name) => name && !name.startsWith('--'))).toEqual(['safe'])
  }
})
