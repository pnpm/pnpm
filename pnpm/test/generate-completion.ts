import { execPnpmSync } from './utils'

test('pnpm generate-completion requires the shell argument', () => {
  const child = execPnpmSync(['generate-completion'])
  const stdout = child.stdout.toString().trim()
  const stderr = child.stderr.toString().trim()
  expect(stdout).toBe('')
  expect(stderr).toBe('missing argument for shell')
  expect(child.status).toBe(1)
})

test('pnpm generate-completion errors on unsupported shell', () => {
  const child = execPnpmSync(['generate-completion', 'weird-shell-nobody-uses'])
  const stdout = child.stdout.toString().trim()
  const stderr = child.stderr.toString().trim()
  expect(stdout).toBe('')
  expect(stderr).toBe('weird-shell-nobody-uses is not supported')
  expect(child.status).toBe(1)
})

for (const shell of ['bash', 'fish', 'zsh']) {
  test(`pnpm generate-completion ${shell}`, () => {
    const child = execPnpmSync(['generate-completion', shell])
    const stdout = child.stdout.toString().trim()
    const stderr = child.stderr.toString().trim()
    expect(stdout).toContain('###-begin-pnpm-completion-###')
    expect(stdout).toContain('###-end-pnpm-completion-###')
    expect(stderr).toBe('')
    expect(child.status).toBe(0)
  })
}
