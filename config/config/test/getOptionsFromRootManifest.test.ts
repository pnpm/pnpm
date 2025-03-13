import path from 'path'
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

test('getOptionsFromRootManifest() should derive allowUnusedPatches from allowNonAppliedPatches (legacy behavior)', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
    },
  })).toStrictEqual({
    allowUnusedPatches: true,
  })
})

test('allowUnusedPatches should override allowNonAppliedPatches', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })
})

test('getOptionsFromRootManifest() should return patchedDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      patchedDependencies: {
        foo: 'foo.patch',
      },
    },
  })
  expect(options.patchedDependencies).toStrictEqual({ foo: path.resolve('foo.patch') })
})
