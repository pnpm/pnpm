import { init } from '@pnpm/plugin-commands-script-runners'
import prepare from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
import * as enquirer from 'enquirer'

// eslint-disable-next-line
const prompt = enquirer.prompt as any

test('pnpm init prints something', async () => {
  prepare({})
  prompt.mockClear()

  await init.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  expect(prompt).toBeCalledWith(expect.objectContaining({
    footer: '\nCtrl-c to cancel.',
    message: 'This utility will walk you through creating a package.json file.\n' +
        `It only covers the most common items, and tries to guess sensible defaults.\n\n`,
  }))
})
