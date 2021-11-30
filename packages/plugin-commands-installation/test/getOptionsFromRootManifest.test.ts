import getOptionsFromRootManifest from '@pnpm/plugin-commands-installation/lib/getOptionsFromRootManifest'

test('getOptionsFromRootManifest() should read "resolutions" field for compatibility with Yarn', () => {
  const options = getOptionsFromRootManifest({
    resolutions: {
      foo: '1.0.0',
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})
