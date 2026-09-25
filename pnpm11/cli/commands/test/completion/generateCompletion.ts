import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { generateCompletion } from '@pnpm/cli.commands'
import { SUPPORTED_SHELLS } from '@pnpm/tabtab'

function createHandler () {
  const log = jest.fn()
  const handler = generateCompletion.createCompletionGenerator({ log })
  return { log, handler }
}

test('pnpm completion requires the shell argument', async () => {
  const { log, handler } = createHandler()
  const promise = handler({}, [])
  await expect(promise).rejects.toMatchObject({
    code: 'ERR_PNPM_MISSING_SHELL_NAME',
    message: '`pnpm completion` requires a shell name',
  })
  expect(log).not.toHaveBeenCalled()
})

test('pnpm completion errors on unsupported shell', async () => {
  const { log, handler } = createHandler()
  const promise = handler({}, ['weird-shell-nobody-uses'])
  await expect(promise).rejects.toMatchObject({
    code: 'ERR_PNPM_UNSUPPORTED_SHELL',
    message: '\'weird-shell-nobody-uses\' is not supported',
  })
  expect(log).not.toHaveBeenCalled()
})

test('pnpm completion errors on redundant parameters', async () => {
  const { log, handler } = createHandler()
  const promise = handler({}, ['bash', 'fish', 'pwsh', 'zsh'])
  await expect(promise).rejects.toMatchObject({
    code: 'ERR_PNPM_REDUNDANT_PARAMETERS',
    message: 'The 3 parameters after shell is not necessary',
  })
  expect(log).not.toHaveBeenCalled()
})

for (const shell of SUPPORTED_SHELLS) {
  test(`pnpm completion ${shell}`, async () => {
    const { log, handler } = createHandler()
    await handler({}, [shell])
    expect(log).toHaveBeenCalledWith(expect.stringContaining('###-begin-pnpm-completion-###'))
    expect(log).toHaveBeenCalledWith(expect.stringContaining('###-end-pnpm-completion-###'))
    expect(log).toHaveBeenCalledTimes(1)
  })
}

test.each([
  ['bash', ['complete -o default -F _pnpm_completion pnpm pn']],
  ['fish', [
    'complete -f -d \'pnpm\' -c pnpm -a "(_pnpm_completion)"',
    'complete -f -d \'pnpm\' -c pn -a "(_pnpm_completion)"',
  ]],
  ['pwsh', ['Register-ArgumentCompleter -CommandName \'pnpm\',\'pn\' -ScriptBlock']],
  ['zsh', [
    '#compdef pnpm pn',
    'compdef _pnpm_completion pnpm pn',
  ]],
])('pnpm completion %s registers the pn alias', async (shell, expectedSnippets) => {
  const { log, handler } = createHandler()
  await handler({}, [shell])
  const output = log.mock.calls[0][0]
  for (const snippet of expectedSnippets) {
    expect(output).toContain(snippet)
  }
})

if (process.platform !== 'win32') {
  test.each(['*literal', 'semi;printf injected', 'dollar$(printf injected)', 'space name', "quote'name", 'tick`name'])('bash completion preserves the literal candidate %s', async (scriptName) => {
    const { log, handler } = createHandler()
    await handler({}, ['bash'])
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-bash-completion-'))
    try {
      fs.writeFileSync(path.join(directory, 'expanded-literal'), '')
      const script = `${log.mock.calls[0][0]}
pnpm () { printf '%s\\n' "$SCRIPT_NAME"; }
COMP_WORDS=(pnpm run "")
COMP_CWORD=2
COMP_LINE="pnpm run "
COMP_POINT=9
_pnpm_completion
eval "set -- \${COMPREPLY[0]}"
printf '%s\\n' "$@"
`
      expect(execFileSync('bash', ['--noprofile', '--norc', '-c', script], { cwd: directory, encoding: 'utf8', env: { ...process.env, SCRIPT_NAME: scriptName } })).toBe(`${scriptName}\n`)
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })

  test.each([
    ['(pnpm run test : u)', 'pnpm run test:u', ['test:unit'], ['unit']],
    ['(pnpm run test :)', 'pnpm run test:', ['test:e2e', 'test:unit'], ['e2e', 'unit']],
    ['(pnpm run test:test : u)', 'pnpm run test:test:u', ['test:test:unit'], ['unit']],
    ['(pnpm install --reporter = app)', 'pnpm install --reporter=app', ['--reporter=append-only'], ['append-only']],
  ])('bash completion replaces only the part of %s after the last word break', async (words, line, candidates, expected) => {
    const { log, handler } = createHandler()
    await handler({}, ['bash'])
    const script = `${log.mock.calls[0][0]}
pnpm () { printf '%s\\n' "$CANDIDATES"; }
COMP_WORDS=${words}
COMP_CWORD=$((\${#COMP_WORDS[@]} - 1))
COMP_LINE='${line}'
COMP_POINT=\${#COMP_LINE}
_pnpm_completion
printf '%s\\n' "\${COMPREPLY[@]}"
`
    expect(execFileSync('bash', ['--noprofile', '--norc', '-c', script], { encoding: 'utf8', env: { ...process.env, CANDIDATES: candidates.join('\n') } })).toBe(`${expected.join('\n')}\n`)
  })
}
