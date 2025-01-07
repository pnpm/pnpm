import { validatePeerDependencies } from '../src/validatePeerDependencies'

test('accepts valid specifications that make sense for peerDependencies', () => {
  validatePeerDependencies({
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        'semver-range': '>=1.2.3 || ^3.2.1',
        'workspace-scheme': 'workspace:^',
        'catalog-scheme': 'catalog:',
      },
    },
  })
})

test('forbids aliases', () => {
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        foo: 'bar@1.2.3',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'/repo/packages/pkg\' has an invalid value: \'bar@1.2.3\'')
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      name: 'my-pkg',
      peerDependencies: {
        foo: 'bar@1.2.3',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'my-pkg\' has an invalid value: \'bar@1.2.3\'')
})

test('forbids `file:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        foo: 'file:../foo',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'/repo/packages/pkg\' has an invalid value: \'file:../foo\'')
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      name: 'my-pkg',
      peerDependencies: {
        foo: 'file:../foo',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'my-pkg\' has an invalid value: \'file:../foo\'')
})

test('forbids `link:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        foo: 'link:../foo',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'/repo/packages/pkg\' has an invalid value: \'link:../foo\'')
  expect(validatePeerDependencies.bind(null, {
    rootDir: '/repo/packages/pkg',
    manifest: {
      name: 'my-pkg',
      peerDependencies: {
        foo: 'link:../foo',
      },
    },
  })).toThrow('The peerDependencies field named \'foo\' of package \'my-pkg\' has an invalid value: \'link:../foo\'')
})
