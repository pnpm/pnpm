import { expect, test } from '@jest/globals'

import { whereRefusedKeyBelongs } from '../lib/index.js'

// The global config file takes these under a name of their own.
test.each([
  ['stateDir', 'state-dir'],
  ['globalDir', 'global-dir'],
  ['globalBinDir', 'global-bin-dir'],
  ['npmrcAuthFile', 'npmrc-auth-file'],
])('%s is set globally as %s', (camelKey, kebabKey) => {
  expect(whereRefusedKeyBelongs(camelKey)).toBe(`Set it for the machine instead: pnpm config set --global ${kebabKey}`)
})

// These are derived from a key the global config file does take, and naming
// their own spelling would send the user to a command that does nothing.
test.each([
  ['bin', 'global-bin-dir'],
  ['globalPkgDir', 'global-dir'],
  ['userconfig', 'npmrc-auth-file'],
])('%s is set globally as %s, the key it derives from', (camelKey, kebabKey) => {
  expect(whereRefusedKeyBelongs(camelKey)).toBe(`Set it for the machine instead: pnpm config set --global ${kebabKey}`)
})

test('dir names the flag that sets it', () => {
  expect(whereRefusedKeyBelongs('dir')).toBe('Pass --dir on the command line instead')
})

// Naming where pnpm gets these would publish how it resolves them, and would
// imply the key was a setting that only reached the wrong file.
test.each([
  'configDir',
  'pnpmHomeDir',
  'rootProjectManifestDir',
  'workspaceDir',
  'authConfig',
  'userConfig',
  'configByUri',
  'packageManagerNetworkConfig',
  'packageManagerRegistries',
])('%s is not offered a route', (camelKey) => {
  expect(whereRefusedKeyBelongs(camelKey)).toBe('This is not a pnpm setting')
})
