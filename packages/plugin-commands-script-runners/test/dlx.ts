import execa from 'execa'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'

jest.mock('execa')

beforeEach((execa as jest.Mock).mockClear)

test('dlx should work with scoped packages', async () => {
  prepareEmpty()

  await dlx.handler({}, ['@foo/bar'])

  expect(execa).toBeCalledWith('bar', [], expect.anything())
})

test('dlx should work with versioned packages', async () => {
  prepareEmpty()

  await dlx.handler({}, ['@foo/bar@next'])

  expect(execa).toBeCalledWith(
    'pnpm',
    expect.arrayContaining(['add', '@foo/bar@next']),
    expect.anything()
  )
  expect(execa).toBeCalledWith('bar', [], expect.anything())
})
