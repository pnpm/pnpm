import updateToLatestSpecsFromManifest, { createLatestSpecs } from '@pnpm/plugin-commands-installation/lib/updateToLatestSpecsFromManifest'

const MAINFEST = {
  dependencies: {
    'dep-of-pkg-with-1-dep': 'npm:pkg-with-1-dep@1.0.0',
  },
  devDependencies: {
    foo: 'github:pnpm/foo',
  },
  optionalDependencies: {
    'is-positive': '^1.0.0',
  },
}

test('updateToLatestSpecsFromManifest()', () => {
  const updateResult1 = expect(updateToLatestSpecsFromManifest(MAINFEST, {
    optionalDependencies: true,
    dependencies: true,
    devDependencies: true,
  }))
  updateResult1.toHaveLength(2)
  updateResult1.toContain('dep-of-pkg-with-1-dep@npm:pkg-with-1-dep@latest')
  updateResult1.toContain('is-positive@latest')

  const updateResult2 = expect(updateToLatestSpecsFromManifest(MAINFEST, {
    optionalDependencies: false,
    dependencies: true,
    devDependencies: false,
  }))
  updateResult2.toHaveLength(1)
  updateResult2.toStrictEqual(['dep-of-pkg-with-1-dep@npm:pkg-with-1-dep@latest'])
})

test('createLatestSpecs()', () => {
  expect(
    createLatestSpecs(['dep-of-pkg-with-1-dep', 'is-positive@2.0.0', 'foo'], MAINFEST)
  ).toStrictEqual(['dep-of-pkg-with-1-dep@npm:pkg-with-1-dep@latest', 'is-positive@2.0.0', 'foo'])

  expect(
    createLatestSpecs(['dep-of-pkg-with-1-dep', 'is-positive', 'foo', 'bar'], MAINFEST)
  ).toStrictEqual(['dep-of-pkg-with-1-dep@npm:pkg-with-1-dep@latest', 'is-positive@latest', 'foo'])
})
