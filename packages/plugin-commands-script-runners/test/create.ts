import PnpmError from '@pnpm/error'
import { create, dlx } from '../src'

jest.mock('../src/dlx', () => ({ handler: jest.fn() }))

beforeEach((dlx.handler as jest.Mock).mockClear)

it('throws an error if called without arguments', async () => {
  await expect(create.handler({}, [])).rejects.toThrow(PnpmError)
  expect(dlx.handler).not.toBeCalled()
})

it(
  'appends `create-` to an unscoped package that doesn\'t start with `create-`',
  async () => {
    await create.handler({}, ['some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['create-some-app'])

    await create.handler({}, ['create_no_dash'])
    expect(dlx.handler).toBeCalledWith({}, ['create-create_no_dash'])
  }
)

it(
  'does not append `create-` to an unscoped package that starts with `create-`',
  async () => {
    await create.handler({}, ['create-some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['create-some-app'])

    await create.handler({}, ['create-'])
    expect(dlx.handler).toBeCalledWith({}, ['create-'])
  }
)

it(
  'appends `create-` to a scoped package that doesn\'t start with `create-`',
  async () => {
    await create.handler({}, ['@scope/some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-some-app'])

    await create.handler({}, ['@scope/create_no_dash'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-create_no_dash'])
  }
)

it(
  'does not append `create-` to a scoped package that starts with `create-`',
  async () => {
    await create.handler({}, ['@scope/create-some-app'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-some-app'])

    await create.handler({}, ['@scope/create-'])
    expect(dlx.handler).toBeCalledWith({}, ['@scope/create-'])
  }
)

it('infers a package name from a plain scope', async () => {
  await create.handler({}, ['@scope'])
  expect(dlx.handler).toBeCalledWith({}, ['@scope/create'])
})

it('passes the remaining arguments to `dlx`', async () => {
  await create.handler({}, ['some-app', 'directory/', '--silent'])
  expect(dlx.handler).toBeCalledWith({}, ['create-some-app', 'directory/', '--silent'])
})
