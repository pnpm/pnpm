import { getShellFromString, getShellFromParams } from './getShell'

test('getShellFromString errors on undefined', () => {
  expect(() => getShellFromString()).toThrow('`pnpm completion` requires a shell name')
})

test('getShellFromString errors on empty input', () => {
  expect(() => getShellFromString('')).toThrow('`pnpm completion` requires a shell name')
})

test('getShellFromString errors on white space', () => {
  expect(() => getShellFromString(' ')).toThrow('`pnpm completion` requires a shell name')
})

test('getShellFromString errors on unsupported shell', () => {
  expect(() => getShellFromString('weird-shell-nobody-uses')).toThrow("'weird-shell-nobody-uses' is not supported")
})

test('getShellFromString returns supported shell as-is', () => {
  expect(getShellFromString('bash')).toBe('bash')
})

test('getShellFromString trims whitespaces on support shell', () => {
  expect(getShellFromString(' bash\n')).toBe('bash')
})

test('getShellFromParams errors on empty input', () => {
  expect(() => getShellFromParams([])).toThrow('`pnpm completion` requires a shell name')
})

test('getShellFromParams errors on redundant parameters', () => {
  expect(() => getShellFromParams(['bash', 'zsh', 'fish', 'pwsh'])).toThrow('The 3 parameters after shell is not necessary')
})

test('getShellFromParams returns supported shell', () => {
  expect(getShellFromParams(['bash'])).toBe('bash')
})
