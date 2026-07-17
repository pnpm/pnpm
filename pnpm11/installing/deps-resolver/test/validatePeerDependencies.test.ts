import { expect, test } from '@jest/globals'

import { validatePeerDependencies } from '../src/validatePeerDependencies.js'

test('accepts valid specifications that make sense for peerDependencies', () => {
  validatePeerDependencies({
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        'semver-range': '>=1.2.3 || ^3.2.1',
        'workspace-scheme': 'workspace:^',
        'catalog-scheme': 'catalog:',
        'combine-all': '>=1.2.3 || ^3.2.1 || workspace:^ || catalog:',
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

test('accepts dependency specifiers that carry a scheme', () => {
  validatePeerDependencies({
    rootDir: '/repo/packages/pkg',
    manifest: {
      peerDependencies: {
        'named-registry': 'work:5.x.x',
        'npm-alias': 'npm:bar@^5',
        'file-scheme': 'file:../foo',
        'link-scheme': 'link:../foo',
        'git-scheme': 'git+https://example.com/foo.git',
      },
    },
  })
})
