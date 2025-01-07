import { validatePeerDependencies } from '../src/validatePeerDependencies'

test('accepts valid specifications that make sense for peerDependencies', () => {
  validatePeerDependencies({
    'use-version-range': '>=1.2.3 || ^3.2.1',
    'use-aliased-version-range': 'foo@>=1.2.3 || ^3.2.1',
    'use-npm-alias': 'npm:pkg@>=1.2.3 || 3.2.1',
    'use-workspace': 'workspace:^',
    'use-workspace-alias': 'pkg@workspace:^',
    'use-catalog': 'catalog:',
    'use-catalog-alias': 'pkg@catalog:',
  })
})

test('forbids `file:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'file:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: file:../foo')
})

test('forbids aliased `file:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'foo@file:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: foo@file:../foo')
})

test('forbids `link:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'link:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: link:../foo')
})

test('forbids aliased `link:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'foo@link:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: foo@link:../foo')
})
