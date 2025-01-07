import { validatePeerDependencies } from '../src/validatePeerDependencies'

test('accepts valid specifications that make sense for peerDependencies', () => {
  validatePeerDependencies({
    'use-version-range': '>=1.2.3 || ^3.2.1',
    'use-workspace': 'workspace:^',
    'use-catalog': 'catalog:',
  })
})

test('forbids aliases', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'bar@1.2.3',
  })).toThrow('The peer dependency named foo has unacceptable specification: bar@1.2.3')
})

test('forbids `file:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'file:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: file:../foo')
})

test('forbids `link:` scheme', () => {
  expect(validatePeerDependencies.bind(null, {
    foo: 'link:../foo',
  })).toThrow('The peer dependency named foo has unacceptable specification: link:../foo')
})
