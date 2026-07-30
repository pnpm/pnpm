import { expect, test } from '@jest/globals'

import { checkSudo } from './checkSudo.js'

const rootUid = () => 0
const userUid = () => 1000
const sudoEnv = { SUDO_USER: 'alice' }

test('allowed when not running as root', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], env: sudoEnv, geteuid: userUid })
  }).not.toThrow()
})

test('allowed for plain root without sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], env: {}, geteuid: rootUid })
  }).not.toThrow()
})

test('allowed when SUDO_USER is root', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], env: { SUDO_USER: 'root' }, geteuid: rootUid })
  }).not.toThrow()
})

test('setup and self-update are blocked under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'setup', cliParams: [], env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm setup" with sudo is not supported')
  expect(() => {
    checkSudo({ cmd: 'self-update', cliParams: [], env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm self-update" with sudo is not supported')
})

test('global add is blocked under sudo with the expected error code', () => {
  let thrownCode: string | undefined
  try {
    checkSudo({ cmd: 'add', cliParams: ['foo'], global: true, env: sudoEnv, geteuid: rootUid })
  } catch (err: any) { // eslint-disable-line
    thrownCode = err.code
  }
  expect(thrownCode).toBe('ERR_PNPM_SUDO_NOT_SUPPORTED')
})

test('local add is allowed under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'add', cliParams: ['foo'], env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
})

test('read-only global commands are allowed under sudo', () => {
  for (const cmd of ['bin', 'root', 'prefix', 'list', 'outdated']) {
    expect(() => {
      checkSudo({ cmd, cliParams: [], global: true, env: sudoEnv, geteuid: rootUid })
    }).not.toThrow()
  }
})

test('config writes are blocked even without an explicit --global because they default to the global config', () => {
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['set', 'store-dir', '/tmp/store'], env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm config set --global" with sudo is not supported')
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['delete', 'store-dir'], global: true, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm config delete --global" with sudo is not supported')
  expect(() => {
    checkSudo({ cmd: 'set', cliParams: ['store-dir=/tmp/store'], env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm config set --global" with sudo is not supported')
})

test('config writes scoped to the project are allowed under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['set', 'store-dir', '/tmp/store'], location: 'project', env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
})

test('config reads are allowed under sudo', () => {
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['get', 'store-dir'], global: true, env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
  expect(() => {
    checkSudo({ cmd: 'config', cliParams: ['list'], env: sudoEnv, geteuid: rootUid })
  }).not.toThrow()
})

test('bare link targets the global directory and is blocked', () => {
  expect(() => {
    checkSudo({ cmd: 'link', cliParams: [], global: true, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm link --global" with sudo is not supported')
})

test('link --global with a package path is a global write and is blocked', () => {
  expect(() => {
    checkSudo({ cmd: 'link', cliParams: ['../foo'], global: true, env: sudoEnv, geteuid: rootUid })
  }).toThrow('Running "pnpm link --global" with sudo is not supported')
})
