import { SUPPORTED_SHELLS } from '@pnpm/tabtab'
import { generateCompletion } from '@pnpm/plugin-commands-completion'

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
