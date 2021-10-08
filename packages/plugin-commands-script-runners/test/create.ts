import { create } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import PnpmError from '@pnpm/error'

jest.mock('../src/dlx', () => ({ handler: jest.fn() }))

// eslint-disable-next-line
import * as dlx from '../src/dlx'

test('throws an error if called without arguments', async () => {
  prepareEmpty();
  (dlx.handler as jest.Mock).mockClear()

  await expect(create.handler({}, [])).rejects.toThrow(PnpmError)
  expect(dlx.handler).not.toBeCalled()
})

test(
  'appends `create-` to an unscoped package that doesn\'t start with `create-`',
  async () => {
    prepareEmpty();
    (dlx.handler as jest.Mock).mockClear()

    await create.handler({}, ['some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['create-some-app'])

    await create.handler({}, ['create_no_dash'])
    expect(dlx.handler).toBeCalledWith({}, ['create-create_no_dash'])
  }
)

test(
  'does not append `create-` to an unscoped package that starts with `create-`',
  async () => {
    prepareEmpty();
    (dlx.handler as jest.Mock).mockClear()

    await create.handler({}, ['create-some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['create-some-app'])

    await create.handler({}, ['create-'])
    expect(dlx.handler).toBeCalledWith({}, ['create-'])
  }
)

test(
  'appends `create-` to a scoped package that doesn\'t start with `create-`',
  async () => {
    prepareEmpty();
    (dlx.handler as jest.Mock).mockClear()

    await create.handler({}, ['@scope/some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-some-app'])

    await create.handler({}, ['@scope/create_no_dash'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-create_no_dash'])
  }
)

test(
  'does not append `create-` to a scoped package that starts with `create-`',
  async () => {
    prepareEmpty();
    (dlx.handler as jest.Mock).mockClear()

    await create.handler({}, ['@scope/create-some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-some-app'])

    await create.handler({}, ['@scope/create-'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-'])
  }
)

test('infers a package name from a plain scope', async () => {
  prepareEmpty();
  (dlx.handler as jest.Mock).mockClear()

  await create.handler({}, ['@scope'])
  expect(dlx.handler).toBeCalledWith({}, ['@scope/create'])
})

test('passes the remaining arguments to `dlx`', async () => {
  prepareEmpty();
  (dlx.handler as jest.Mock).mockClear()

  await create.handler({}, ['some-app', 'directory/', '--', '--silent'])
  expect(dlx.handler).toBeCalledWith({}, ['create-some-app', 'directory/', '--silent'])
})
