import PnpmError from '@pnpm/error'
import { init } from '../src'

// mock the file detection and writing functions

describe('non-interactive mode', () => {
  it('generates a valid package.json in the current directory', async () => {
    await init.handler({}, [])

    // check that the file was created
  })

  it('throws an error if a package.json exists in the current directory', async () => {
    // mock the existence of ./package.json

    await expect(
      init.handler({}, [])
    ).rejects.toThrow(PnpmError)

    // check that the file wasn't created
  })

  it('throws an error on excess arguments', async () => {
    await expect(
      init.handler({}, ['directory', 'something-else'])
    ).rejects.toThrow(PnpmError)

    // check that the file wasn't created
  })

  it('overwrites the existing package.json in the current directory with `--force`', async () => {
    // mock the existence of ./package.json

    await init.handler({ force: true }, [])

    // check that the file was overwritten
  })

  it('creates and respects the specified directory', async () => {
    await init.handler({}, ['directory'])

    // check that the directory and file were created
  })
})

describe('interactive mode', () => {
  it('generates a valid package.json in the current directory', async () => {
    // mock the prompt

    await init.handler({ interactive: true }, [])

    // check that the file was created
  })

  it('respects the user\'s edits', async () => {
    // mock the prompt

    await init.handler({ interactive: true }, [])

    // check that the file was created
  })

  it('screens quotes in user input', async () => {
    // mock the prompt

    await init.handler({ interactive: true }, [])

    // check that the file was created
  })

  it('throws an error if a package.json exists in the current directory', async () => {
    // mock the existence of ./package.json

    await init.handler({ interactive: true }, [])

    // check that the prompt wasn't called

    // check that the file wasn't created
  })

  it('overwrites the existing package.json in the current directory with `--force`', async () => {
    // mock the existence of ./package.json

    // mock the prompt

    await init.handler({ interactive: true, force: true }, [])

    // check that the file was created
  })

  it('creates and respects the specified directory', async () => {
    // mock the prompt

    await init.handler({ interactive: true }, ['directory'])

    // check that the directory and file were created
  })
})
