import { expect, test } from '@jest/globals'
import {
  escapesWorkspaceRoot,
  isHostFilesystemSpecifier,
  isNonLocalFilesystemSubdepSpec,
  stripPackageNameFromLocalSpec,
} from '@pnpm/resolving.local-resolver'

test('isHostFilesystemSpecifier classifies local and escaping workspace specifiers', () => {
  for (const localSpec of [
    'file:/path/on/server',
    'file:../dir',
    'file:./local.tgz',
    'file:///path/on/server',
    'FILE:/path/on/server',
    'link:/path/on/server',
    'link:../dir',
    'git+file:/path/on/server',
    './foo',
    '../foo',
    '/foo',
    '~/foo',
    '\\foo',
    'C:/foo',
    'C:\\foo',
    'C:tools',
    'packages/foo/bar',
    'a/../../outside',
    'workspace:/path/on/server',
    'workspace:../outside',
    'workspace:~/outside',
    'npm:pkg@file:../outside',
    'lib@file:///workspace/packages/lib',
    '@scope/lib@link:../lib',
  ]) {
    expect(isHostFilesystemSpecifier(localSpec, '.', () => false)).toBe(true)
  }

  for (const nonLocalSpec of [
    '^1.0.0',
    '~1.2.3',
    '~',
    '*',
    'latest',
    'npm:lodash@^4.17.21',
    'jsr:@std/path@^1.0.0',
    'catalog:',
    'workspace:*',
    'workspace:~',
    'workspace:^1.0.0',
    'workspace:@scope/pkg@^1.0.0',
    'workspace:./packages/target',
    'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
    'git+https://github.com/org/repo.git#main',
    'github:org/repo',
    'org/repo',
    'lib@1.0.0',
    '@scope/lib@^1.0.0',
  ]) {
    expect(isHostFilesystemSpecifier(nonLocalSpec, '.', () => false)).toBe(false)
  }

  expect(isHostFilesystemSpecifier('workspace:../lib', 'packages/app', () => false)).toBe(false)
  expect(isHostFilesystemSpecifier('workspace:../../../outside', 'packages/app', () => false)).toBe(true)
  expect(isHostFilesystemSpecifier('c:pkg@1.0.0', '.', (prefix) => prefix === 'c')).toBe(false)
  expect(isHostFilesystemSpecifier('c:/pkg', '.', (prefix) => prefix === 'c')).toBe(true)
})

test('isNonLocalFilesystemSubdepSpec rejects host-filesystem specs and preserves named workspace ranges', () => {
  for (const forbidden of [
    'file:~/.config/gh',
    'file:/etc',
    'file:../secret',
    'file:./sub.tgz',
    'file:a/../../b',
    'file:C:\\secret',
    'FILE:../secret',
    'link:~/.config/gh',
    'link:../secret',
    'path:../secret',
    'npm:alias@file:~/.config/gh',
    'npm:outer@npm:inner@link:../secret',
    'workspace:../secret',
    'workspace:./packages/a',
    'workspace:/etc',
    'workspace:~/.config/gh',
    'workspace:~\\.config\\gh',
    'workspace:C:\\secret',
    'workspace:pkg@file:../secret',
    'git+file:///tmp/repo',
    'GIT+FILE:///tmp/repo',
    'npm:alias@git+file:///tmp/repo',
    '../secret',
    './secret',
    '/etc',
    '\\etc',
    'file:~',
    '~/.config/gh',
    '~\\.config\\gh',
    'C:/secret',
    'C:\\secret',
    'C:secret',
    'a/b/c',
    'a/../../secret',
    'host@file:~/.config/gh',
    '@scope/host@link:../secret',
  ]) {
    expect(isNonLocalFilesystemSubdepSpec(forbidden, () => false)).toBe(true)
  }

  for (const allowed of [
    '^1.0.0',
    '~1.2.3',
    '~',
    '*',
    'latest',
    'npm:lodash@^4.17.21',
    'jsr:@std/path@^1.0.0',
    'workspace:*',
    'workspace:^',
    'workspace:~',
    'workspace:^1.0.0',
    'workspace:@scope/pkg@~',
    'https://example.com/pkg.tgz',
    'git+https://github.com/org/repo.git',
    'github:org/repo',
    'org/repo',
  ]) {
    expect(isNonLocalFilesystemSubdepSpec(allowed, () => false)).toBe(false)
  }

  expect(isNonLocalFilesystemSubdepSpec('c:pkg@1.0.0', (prefix) => prefix === 'c')).toBe(false)
  expect(isNonLocalFilesystemSubdepSpec('c:/pkg', (prefix) => prefix === 'c')).toBe(true)

  expect(escapesWorkspaceRoot('.', '../secret')).toBe(true)
  expect(escapesWorkspaceRoot('.', 'a/../../secret')).toBe(true)
  expect(escapesWorkspaceRoot('.', '/etc')).toBe(true)
  expect(escapesWorkspaceRoot('.', '~/.config/gh')).toBe(true)
  expect(escapesWorkspaceRoot('.', 'packages/a')).toBe(false)
  expect(escapesWorkspaceRoot('packages/a', '../../inside')).toBe(false)
  expect(escapesWorkspaceRoot('packages/a', '../../../outside')).toBe(true)
  expect(escapesWorkspaceRoot('packages/../app', '../../secret')).toBe(true)
  expect(escapesWorkspaceRoot('packages/../app', '../inside')).toBe(false)
  expect(escapesWorkspaceRoot('../outside', 'inside')).toBe(true)
})

test('stripPackageNameFromLocalSpec reads only a package name followed by a local specifier', () => {
  expect(stripPackageNameFromLocalSpec('lib@file:///workspace/lib')).toBe('file:///workspace/lib')
  expect(stripPackageNameFromLocalSpec('@scope/lib@link:../lib')).toBe('link:../lib')
  expect(stripPackageNameFromLocalSpec('lib@path:../lib')).toBe('path:../lib')
  for (const notKeyShaped of [
    '',
    '@',
    'lib@1.0.0',
    '@scope/lib@^1.0.0',
    '@scope@file:../lib',
    'a/b@file:../lib',
    'npm:lib@file:../lib',
    './lib@file:../lib',
    'file:../lib@file:../other',
  ]) {
    expect(stripPackageNameFromLocalSpec(notKeyShaped)).toBeUndefined()
  }
})
