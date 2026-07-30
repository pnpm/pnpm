import { expect, test } from '@jest/globals'

import { checkSudo } from './checkSudo.js'

const rootUid = () => 0
const userUid = () => 1000
const sudoEnv = { SUDO_USER: 'alice' }

test('allowed when not running as root', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], isGlobal: false, env: sudoEnv, geteuid: userUid })
  }).not.toThrow()
})

test('allowed for plain root without sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], isGlobal: false, env: {}, geteuid: rootUid })
  }).not.toThrow()
})

test('allowed when root sudoed to itself', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], isGlobal: false, env: { SUDO_USER: 'root' }, geteuid: rootUid })
  }).not.toThrow()
})

test('setup and self-update are blocked under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], isGlobal: false, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm setup" with sudo is not supported')
  expect(() => {
    checkSudo({ cmd: 'self-update', cliParams: [], isGlobal: false, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm self-update" with sudo is not supported')
})

test('global add is blocked under sudo with the expected error code', () => {
  let thrownCode: string | undefined
  try {
    checkSudo({ cmd: 'add', cliParams: ['foo'], isGlobal: true, env: sudoEnv, geteuid: rootUid })
  } catch (err: any) { // eslint-disable-line
    thrownCode = err.code
  }
  expect(thrownCode).toBe('ERR_PNPM_SUDO_NOT_SUPPORTED')
})

test('local add is allowed under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'add', cliParams: ['foo'], isGlobal: false, env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
})

test('read-only global commands are allowed under sudo', () => {
  for (const cmd of ['bin', 'root', 'prefix', 'list', 'outdated']) {
    expect(() => {
      checkSudo({ cmd, cliParams: [], isGlobal: true, env: sudoEnv, geteuid: rootUid })
    }).not.toThrow()
  }
})

test('global config writes are blocked but reads allowed', () => {
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['set', 'store-dir', '/tmp/store'], isGlobal: true, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm config set --global" with sudo is not supported')
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['get', 'store-dir'], isGlobal: true, env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
})

test('bare link targets the global directory and is blocked', () => {
  expect(() => {
    checkSudo({ cmd: 'link', cliParams: [], isGlobal: true, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm link --global" with sudo is not supported')
})
