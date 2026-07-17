import { expect, test } from '@jest/globals'
import { getPeerVersionRange, isAcceptablePeerSpec, isValidPeerRange } from '@pnpm/deps.peer-range'

test('isValidPeerRange only accepts semver ranges and workspace:/catalog: specs', () => {
  expect(isValidPeerRange('^1.0.0')).toBe(true)
  expect(isValidPeerRange('workspace:^')).toBe(true)
  expect(isValidPeerRange('catalog:')).toBe(true)
  expect(isValidPeerRange('work:5.x.x')).toBe(false)
  expect(isValidPeerRange('npm:bar@^5')).toBe(false)
  expect(isValidPeerRange('file:../foo')).toBe(false)
})

test('isAcceptablePeerSpec accepts valid ranges and scheme-carrying specifiers', () => {
  expect(isAcceptablePeerSpec('^1.0.0')).toBe(true)
  expect(isAcceptablePeerSpec('workspace:^')).toBe(true)
  expect(isAcceptablePeerSpec('catalog:')).toBe(true)
  expect(isAcceptablePeerSpec('work:5.x.x')).toBe(true)
  expect(isAcceptablePeerSpec('npm:bar@^5')).toBe(true)
  expect(isAcceptablePeerSpec('file:../foo')).toBe(true)
  expect(isAcceptablePeerSpec('git+https://example.com/foo.git')).toBe(true)
})

test('isAcceptablePeerSpec rejects bare name@version typos', () => {
  expect(isAcceptablePeerSpec('bar@1.2.3')).toBe(false)
  expect(isAcceptablePeerSpec('latest')).toBe(false)
  expect(isAcceptablePeerSpec('not a range')).toBe(false)
})

test('getPeerVersionRange keeps valid peer ranges unchanged', () => {
  expect(getPeerVersionRange('^1.0.0')).toBe('^1.0.0')
  expect(getPeerVersionRange('>=1.2.3 || ^3.2.1')).toBe('>=1.2.3 || ^3.2.1')
  expect(getPeerVersionRange('catalog:')).toBe('catalog:')
})

test('getPeerVersionRange strips a leading workspace: prefix', () => {
  expect(getPeerVersionRange('workspace:^')).toBe('^')
  expect(getPeerVersionRange('workspace:1.2.3')).toBe('1.2.3')
  expect(getPeerVersionRange('workspace:*')).toBe('*')
})

test('getPeerVersionRange extracts the semver body from named-registry and npm: specifiers', () => {
  expect(getPeerVersionRange('work:5.x.x')).toBe('5.x.x')
  expect(getPeerVersionRange('work:^5.0.0')).toBe('^5.0.0')
  expect(getPeerVersionRange('npm:bar@^5')).toBe('^5')
  expect(getPeerVersionRange('npm:@scope/bar@~2.1.0')).toBe('~2.1.0')
  expect(getPeerVersionRange('npm:^5.0.0')).toBe('^5.0.0')
})

test('getPeerVersionRange falls back to * for specifiers without a comparable version', () => {
  expect(getPeerVersionRange('file:../foo')).toBe('*')
  expect(getPeerVersionRange('link:../foo')).toBe('*')
  expect(getPeerVersionRange('git+https://example.com/foo.git')).toBe('*')
  expect(getPeerVersionRange('npm:bar')).toBe('*')
  expect(getPeerVersionRange('work:@scope/bar')).toBe('*')
})
