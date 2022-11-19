import { getOptionsFromRootManifest } from '../lib/getOptionsFromRootManifest'

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
