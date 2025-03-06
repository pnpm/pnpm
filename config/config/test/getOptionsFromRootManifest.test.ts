import { getOptionsFromRootManifest } from '../lib/getOptionsFromRootManifest'

test('getOptionsFromRootManifest() should read "resolutions" field for compatibility with Yarn', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    resolutions: {
      foo: '1.0.0',
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() should read "overrides" field', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      overrides: {
        foo: '1.0.0',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by dependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
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
  const options = getOptionsFromRootManifest(process.cwd(), {
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
  const options = getOptionsFromRootManifest(process.cwd(), {
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
  expect(() => getOptionsFromRootManifest(process.cwd(), {
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

test('getOptionsFromRootManifest() should return onlyBuiltDependencies as undefined by default', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {})
  expect(options.onlyBuiltDependencies).toStrictEqual(undefined)
})

test('getOptionsFromRootManifest() should return the list from onlyBuiltDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      onlyBuiltDependencies: ['electron'],
    },
  })
  expect(options.onlyBuiltDependencies).toStrictEqual(['electron'])
})

test('getOptionsFromRootManifest() should derive allowPatchFailure and allowUnusedPatch from strictPatches', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      strictPatches: true,
    },
  })).toMatchObject({
    allowPatchFailure: false,
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      strictPatches: false,
    },
  })).toMatchObject({
    allowPatchFailure: true,
    allowUnusedPatches: true,
  })
})

test('getOptionsFromRootManifest() should derive allowUnusedPatches from allowNonAppliedPatches', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
    },
  })).toMatchObject({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
    },
  })).toMatchObject({
    allowUnusedPatches: true,
  })
})

describe('strictPatches, when defined, should override allowNonAppliedPatches', () => {
  test.each([
    [undefined, false, false], // `strictPatches` is undefined, use `allowNonAppliedPatches`
    [false, false, true], // `strictPatches` is defined, use `!strictPatches`
    [true, false, false], // `strictPatches` is defined, use `!strictPatches`
    [undefined, true, true], // `strictPatches` is undefined, use `allowNonAppliedPatches`
    [false, true, true], // `strictPatches` is defined, use `!strictPatches`
    [true, true, false], // `strictPatches` is defined, use `!strictPatches`
  ])('{ strictPatches: %o, allowNonAppliedPatches: %o } → { allowUnusedPatches: %o }', (strictPatches, allowNonAppliedPatches, allowUnusedPatches) => {
    expect(getOptionsFromRootManifest(process.cwd(), {
      pnpm: {
        strictPatches,
        allowNonAppliedPatches,
      },
    })).toMatchObject({ allowUnusedPatches })
  })
})
