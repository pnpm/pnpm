import { createInstallArgs } from '../src/runDepsStatusCheck'

describe('createInstallArgs', () => {
  test.each([
    [{ production: true, optional: true }, ['--production']],
    [{ production: true, optional: false }, ['--production', '--no-optional']],
    [{ dev: true, optional: true }, ['--dev']],
    [{ dev: true, optional: false }, ['--dev', '--no-optional']],
    [{ production: true, dev: true, optional: true }, []],
    [{ production: true, dev: true, optional: false }, ['--no-optional']],
  ])('%o -> %o', (opts, expected) => {
    expect(createInstallArgs(opts)).toStrictEqual(expected)
  })
})
