import createPeerDependencyPatcher from '@pnpm/core/lib/install/createPeerDependencyPatcher'

test('createPeerDependencyPatcher() ignores missing', () => {
  const patcher = createPeerDependencyPatcher({
    ignoreMissing: ['foo'],
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '*',
      bar: '*',
    },
  })
  expect(patchedPkg['peerDependenciesMeta']).toStrictEqual({
    foo: {
      optional: true,
    },
  })
})

test('createPeerDependencyPatcher() extends peer ranges', () => {
  const patcher = createPeerDependencyPatcher({
    allowedVersions: {
      foo: '1',
      qar: '1',
    },
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '0',
      bar: '0',
      qar: '*',
    },
  })
  expect(patchedPkg['peerDependencies']).toStrictEqual({
    foo: '0 || 1',
    bar: '0',
    qar: '*',
  })
})
