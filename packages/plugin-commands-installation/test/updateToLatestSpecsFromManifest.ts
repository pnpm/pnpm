import updateToLatestSpecsFromManifest, { createLatestSpecs } from '../lib/updateToLatestSpecsFromManifest'

const MANIFEST = {
  dependencies: {
    '@pnpm.e2e/dep-of-pkg-with-1-dep': 'npm:@pnpm.e2e/pkg-with-1-dep@1.0.0',
  },
  devDependencies: {
    foo: 'github:pnpm/foo',
  },
  optionalDependencies: {
    'is-positive': '^1.0.0',
  },
}

test('updateToLatestSpecsFromManifest()', () => {
  const updateResult1 = expect(updateToLatestSpecsFromManifest(MANIFEST, {
    optionalDependencies: true,
    dependencies: true,
    devDependencies: true,
  }))
  updateResult1.toHaveLength(2)
  updateResult1.toContain('@pnpm.e2e/dep-of-pkg-with-1-dep@npm:@pnpm.e2e/pkg-with-1-dep@latest')
  updateResult1.toContain('is-positive@latest')

  const updateResult2 = expect(updateToLatestSpecsFromManifest(MANIFEST, {
    optionalDependencies: false,
    dependencies: true,
    devDependencies: false,
  }))
  updateResult2.toHaveLength(1)
  updateResult2.toStrictEqual(['@pnpm.e2e/dep-of-pkg-with-1-dep@npm:@pnpm.e2e/pkg-with-1-dep@latest'])
})

test('createLatestSpecs()', () => {
  expect(
    createLatestSpecs(['@pnpm.e2e/dep-of-pkg-with-1-dep', 'is-positive@2.0.0', 'foo'], MANIFEST)
  ).toStrictEqual(['@pnpm.e2e/dep-of-pkg-with-1-dep@npm:@pnpm.e2e/pkg-with-1-dep@latest', 'is-positive@2.0.0', 'foo'])

  expect(
    createLatestSpecs(['@pnpm.e2e/dep-of-pkg-with-1-dep', 'is-positive', 'foo', 'bar'], MANIFEST)
  ).toStrictEqual(['@pnpm.e2e/dep-of-pkg-with-1-dep@npm:@pnpm.e2e/pkg-with-1-dep@latest', 'is-positive@latest', 'foo'])
})
