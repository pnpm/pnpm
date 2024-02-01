import { execPnpmSync } from './utils'

test('pnpm completion requires the shell argument', () => {
  const child = execPnpmSync(['completion'])
  const stdout = child.stdout.toString().trim()
  const stderr = child.stderr.toString().trim()
  expect(stdout).toBe('')
  expect(stderr).toBe('missing argument for shell')
  expect(child.status).toBe(1)
})

test('pnpm completion errors on unsupported shell', () => {
  const child = execPnpmSync(['completion', 'weird-shell-nobody-uses'])
  const stdout = child.stdout.toString().trim()
  const stderr = child.stderr.toString().trim()
  expect(stdout).toBe('')
  expect(stderr).toBe('weird-shell-nobody-uses is not supported')
  expect(child.status).toBe(1)
})

for (const shell of ['bash', 'fish', 'pwsh', 'zsh']) {
  test(`pnpm completion ${shell}`, () => {
    const child = execPnpmSync(['completion', shell])
    const stdout = child.stdout.toString().trim()
    const stderr = child.stderr.toString().trim()
    expect(stdout).toContain('###-begin-pnpm-completion-###')
    expect(stdout).toContain('###-end-pnpm-completion-###')
    expect(stderr).toBe('')
    expect(child.status).toBe(0)
  })
}
