import execa from 'execa'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'

jest.mock('execa')

test('dlx should work with scoped packages', async () => {
  prepareEmpty()

  await dlx.handler({}, ['@foo/bar'])

  expect(execa).toBeCalledWith('bar', [], expect.anything())
})
