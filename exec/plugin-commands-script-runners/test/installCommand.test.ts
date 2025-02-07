import { type Flags, type InstallCommand, createFromFlags } from '../src/installCommand'

describe(createFromFlags, () => {
  test.each([
    [{ production: true, optional: true }, ['install', '--production']],
    [{ production: true, optional: false }, ['install', '--production', '--no-optional']],
    [{ dev: true, optional: true }, ['install', '--dev']],
    [{ dev: true, optional: false }, ['install', '--dev', '--no-optional']],
    [{ production: true, dev: true, optional: true }, ['install']],
    [{ production: true, dev: true, optional: false }, ['install', '--no-optional']],
  ] as Array<[Flags, InstallCommand]>)('%o -> %o', (flags, expected) => {
    expect(createFromFlags(flags)).toStrictEqual(expected)
  })
})
