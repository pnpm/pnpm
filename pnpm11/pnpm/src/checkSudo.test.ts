import { expect, test } from '@jest/globals'

import { sudoBlockedOperation } from './checkSudo.js'

const rootUid = () => 0
const userUid = () => 1000
const sudoEnv = { SUDO_USER: 'alice' }

test('nothing reported when not running as root', () => {
  expect(sudoBlockedOperation({ cmd: 'setup', cliParams: [], env: sudoEnv, geteuid: userUid })).toBeUndefined()
})

test('nothing reported for plain root without sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'setup', cliParams: [], env: {}, geteuid: rootUid })).toBeUndefined()
})

test('nothing reported when SUDO_USER is root', () => {
  expect(sudoBlockedOperation({ cmd: 'setup', cliParams: [], env: { SUDO_USER: 'root' }, geteuid: rootUid })).toBeUndefined()
})

test('setup and self-update are reported under sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'setup', cliParams: [], env: sudoEnv, geteuid: rootUid })).toBe('pnpm setup')
  expect(sudoBlockedOperation({ cmd: 'self-update', cliParams: [], env: sudoEnv, geteuid: rootUid })).toBe('pnpm self-update')
})

test('global add is reported under sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'add', cliParams: ['foo'], global: true, env: sudoEnv, geteuid: rootUid })).toBe('pnpm add --global')
})

test('local add is not reported under sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'add', cliParams: ['foo'], env: sudoEnv, geteuid: rootUid })).toBeUndefined()
})

test('read-only global commands are not reported under sudo', () => {
  for (const cmd of ['bin', 'root', 'prefix', 'list', 'outdated']) {
    expect(sudoBlockedOperation({ cmd, cliParams: [], global: true, env: sudoEnv, geteuid: rootUid })).toBeUndefined()
  }
})

test('config writes are reported even without an explicit --global because they default to the global config', () => {
  expect(sudoBlockedOperation({ cmd: 'config', cliParams: ['set', 'store-dir', '/tmp/store'], env: sudoEnv, geteuid: rootUid }))
    .toBe('pnpm config set --global')
  expect(sudoBlockedOperation({ cmd: 'config', cliParams: ['delete', 'store-dir'], global: true, env: sudoEnv, geteuid: rootUid }))
    .toBe('pnpm config delete --global')
  expect(sudoBlockedOperation({ cmd: 'set', cliParams: ['store-dir=/tmp/store'], env: sudoEnv, geteuid: rootUid }))
    .toBe('pnpm config set --global')
})

test('config writes scoped to the project are not reported under sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'config', cliParams: ['set', 'store-dir', '/tmp/store'], location: 'project', env: sudoEnv, geteuid: rootUid }))
    .toBeUndefined()
})

test('config reads are not reported under sudo', () => {
  expect(sudoBlockedOperation({ cmd: 'config', cliParams: ['get', 'store-dir'], global: true, env: sudoEnv, geteuid: rootUid })).toBeUndefined()
  expect(sudoBlockedOperation({ cmd: 'config', cliParams: ['list'], env: sudoEnv, geteuid: rootUid })).toBeUndefined()
})

test('bare link targets the global directory and is reported', () => {
  expect(sudoBlockedOperation({ cmd: 'link', cliParams: [], global: true, env: sudoEnv, geteuid: rootUid })).toBe('pnpm link --global')
})

test('link --global with a package path is a global write and is reported', () => {
  expect(sudoBlockedOperation({ cmd: 'link', cliParams: ['../foo'], global: true, env: sudoEnv, geteuid: rootUid })).toBe('pnpm link --global')
})
