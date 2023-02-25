import { ProjectManifest } from '@pnpm/types'
import { createPeerDependencyPatcher } from '../lib/createPeerDependencyPatcher'

test('createPeerDependencyPatcher() ignores missing', () => {
  const patcher = createPeerDependencyPatcher({
    ignoreMissing: ['foo'],
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '*',
      bar: '*',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependenciesMeta).toStrictEqual({
    foo: {
      optional: true,
    },
  })
})

test('createPeerDependencyPatcher() pattern matches to ignore missing', () => {
  const patcher = createPeerDependencyPatcher({
    ignoreMissing: ['f*r'],
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foobar: '*',
      bar: '*',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependenciesMeta).toStrictEqual({
    foobar: {
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
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    foo: '0 || 1',
    bar: '0',
    qar: '*',
    baz: '*',
  })
})

test('createPeerDependencyPatcher() ignores peer versions from allowAny', () => {
  const patcher = createPeerDependencyPatcher({
    allowAny: ['foo', 'bar'],
  })
  const patchedPkg = patcher({
    peerDependencies: {
      foo: '2',
      bar: '2',
      qar: '2',
      baz: '2',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    foo: '*',
    bar: '*',
    qar: '2',
    baz: '2',
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
  const patchedAgainPkg = patcher(await patchedPkg) as ProjectManifest
  expect(patchedAgainPkg.peerDependencies).toStrictEqual({
    // the patch is applied only once (not 0 || 1 || 1)
    foo: '0 || 1',
    same: '12',
    multi: '16 || 17',
    mix: '1 || 4 || 2 || 3',
    partialmatch: '16 || 1.2.1 || 1',
    nopadding: '15.0.1 || 16 || ^17.0.1 || 18.x',
  })
})

test('createPeerDependencyPatcher() overrides peerDependencies when parent>child selector is used', () => {
  const patcher = createPeerDependencyPatcher({
    allowedVersions: {
      bar: '2',
      'foo>bar': '1',
      'foo@2>bar': '2 || 3',
      'foo@>=2.3.5 <3>bar': '4',
    },
  })
  let patchedPkg = patcher({
    name: 'foo',
    peerDependencies: {
      bar: '0 || 1',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    bar: '0 || 1',
  })

  patchedPkg = patcher({
    name: 'foo',
    version: '2',
    peerDependencies: {
      bar: '0 || 1',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    bar: '0 || 1 || 2 || 3',
  })

  patchedPkg = patcher({
    name: 'foo',
    version: '3',
    peerDependencies: {
      bar: '0 || 1',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    bar: '0 || 1',
  })

  patchedPkg = patcher({
    name: 'foo',
    version: '2.3.5',
    peerDependencies: {
      bar: '0 || 1',
    },
  }) as ProjectManifest
  expect(patchedPkg.peerDependencies).toStrictEqual({
    bar: '0 || 1 || 4',
  })
})

test('createPeerDependencyPathcer() throws expected error if parent>child selector cannot parse', () => {
  expect(() => createPeerDependencyPatcher({
    allowedVersions: {
      'foo > bar': '2',
    },
  })).toThrowError('Cannot parse the "foo > bar" selector in pnpm.peerDependencyRules.allowedVersions')
})
