import { getOptionsFromRootManifest } from '../lib/getOptionsFromRootManifest'
import { logger } from '@pnpm/logger'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

test('getOptionsFromRootManifest() should read "resolutions" field for compatibility with Yarn', () => {
  const options = getOptionsFromRootManifest({
    resolutions: {
      foo: '1.0.0',
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() should read "overrides" field', () => {
  const options = getOptionsFromRootManifest({
    pnpm: {
      overrides: {
        foo: '1.0.0',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by dependencies', () => {
  const options = getOptionsFromRootManifest({
    dependencies: {
      foo: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by devDependencies', () => {
  const options = getOptionsFromRootManifest({
    devDependencies: {
      foo: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by dependencies and devDependencies', () => {
  const options = getOptionsFromRootManifest({
    dependencies: {
      foo: '1.0.0',
    },
    devDependencies: {
      foo: '2.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() throws an error if cannot resolve an override version reference', () => {
  expect(() => getOptionsFromRootManifest({
    dependencies: {
      bar: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })).toThrow('Cannot resolve version $foo in overrides. The direct dependencies don\'t have dependency "foo".')
})

test('getOptionsFromRootManifest() throws an error if cannot resolve an override link to relative path', () => {
  getOptionsFromRootManifest({
    dependencies: {
      bar: '1.0.0',
    },
    pnpm: {
      overrides: {
        bar: 'link:../test/bar',
      },
    },
  })
  expect(logger.warn).toBeCalledTimes(1)
})

test('getOptionsFromRootManifest() throws an error if cannot resolve an override link to absolute path', () => {
  getOptionsFromRootManifest({
    dependencies: {
      bar: '1.0.0',
    },
    pnpm: {
      overrides: {
        bar: 'link:G:/test/bar',
      },
    },
  })
  expect(logger.warn).toBeCalledTimes(1)
})