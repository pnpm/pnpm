import createPeerDependencyPatcher from '../../lib/install/createPeerDependencyPatcher'

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
      baz: '*',
    },
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '0',
      bar: '0',
      qar: '*',
      baz: '1',
    },
  })
  expect(patchedPkg['peerDependencies']).toStrictEqual({
    foo: '0 || 1',
    bar: '0',
    qar: '*',
    baz: '*',
  })
})

test('createPeerDependencyPatcher() does not create duplicate extended ranges', async () => {
  const patcher = createPeerDependencyPatcher({
    allowedVersions: {
      foo: '1',
      same: '12',
      multi: '16',
      mix: '1 || 2 || 3',
      partialmatch: '1',
      nopadding: '^17.0.1||18.x',
    },
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '0',
      same: '12',
      multi: '16 || 17',
      mix: '1 || 4',
      partialmatch: '16 || 1.2.1',
      nopadding: '15.0.1||16',
    },
  })
  // double apply the same patch to the same package
  // this can occur in a monorepo when several packages
  // all try to apply the same patch
  const patchedAgainPkg = patcher(await patchedPkg)
  expect(patchedAgainPkg['peerDependencies']).toStrictEqual({
    // the patch is applied only once (not 0 || 1 || 1)
    foo: '0 || 1',
    same: '12',
    multi: '16 || 17',
    mix: '1 || 4 || 2 || 3',
    partialmatch: '16 || 1.2.1 || 1',
    nopadding: '15.0.1 || 16 || ^17.0.1 || 18.x',
  })
})
