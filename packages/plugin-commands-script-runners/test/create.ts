import PnpmError from '@pnpm/error'
import { create, dlx } from '../src'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils'

jest.mock('../src/dlx', () => ({ handler: jest.fn() }))

beforeEach((dlx.handler as jest.Mock).mockClear)

it('throws an error if called without arguments', async () => {
  await expect(create.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, [])).rejects.toThrow(PnpmError)
  expect(dlx.handler).not.toBeCalled()
})

it(
  'appends `create-` to an unscoped package that doesn\'t start with `create-`',
  async () => {
    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['some-app'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['create-some-app'])

    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['create_no_dash'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['create-create_no_dash'])
  }
)

it(
  'does not append `create-` to an unscoped package that starts with `create-`',
  async () => {
    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['create-some-app'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['create-some-app'])

    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['create-'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['create-'])
  }
)

it(
  'appends `create-` to a scoped package that doesn\'t start with `create-`',
  async () => {
    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['@scope/some-app'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['@scope/create-some-app'])

    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['@scope/create_no_dash'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['@scope/create-create_no_dash'])
  }
)

it(
  'does not append `create-` to a scoped package that starts with `create-`',
  async () => {
    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['@scope/create-some-app'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['@scope/create-some-app'])

    await create.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['@scope/create-'])
    expect(dlx.handler).toBeCalledWith(expect.anything(), ['@scope/create-'])
  }
)

it('infers a package name from a plain scope', async () => {
  await create.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@scope'])
  expect(dlx.handler).toBeCalledWith(expect.anything(), ['@scope/create'])
})

it('passes the remaining arguments to `dlx`', async () => {
  await create.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['some-app', 'directory/', '--silent'])
  expect(dlx.handler).toBeCalledWith(expect.anything(), ['create-some-app', 'directory/', '--silent'])
})
