import os from 'node:os'

import { expect, jest, test } from '@jest/globals'

import { getHomedir } from '../lib/homedir.js'

const testOnLinux = process.platform === 'linux' ? test : test.skip

test('getHomedir() returns os.homedir() when SUDO_USER is not set', () => {
  expect(getHomedir({}, 'linux')).toBe(os.homedir())
})

test('getHomedir() ignores SUDO_USER when it is root', () => {
  expect(getHomedir({ SUDO_USER: 'root' }, 'linux')).toBe(os.homedir())
})

test('getHomedir() ignores SUDO_USER on platforms without a resolver', () => {
  expect(getHomedir({ SUDO_USER: 'someone' }, 'win32')).toBe(os.homedir())
})

// cspell:disable-next-line
testOnLinux('getHomedir() resolves the SUDO_USER home directory via getent', () => {
  const getuidSpy = jest.spyOn(process, 'getuid').mockReturnValue(0)
  try {
    const user = os.userInfo().username
    expect(getHomedir({ SUDO_USER: user }, 'linux')).toBe(os.userInfo().homedir)
  } finally {
    getuidSpy.mockRestore()
  }
})

testOnLinux('getHomedir() throws when SUDO_USER cannot be resolved', () => {
  const getuidSpy = jest.spyOn(process, 'getuid').mockReturnValue(0)
  try {
    expect(() => getHomedir({ SUDO_USER: 'pnpm-test-nonexistent-user' }, 'linux'))
      .toThrow(/Failed to resolve home directory for SUDO_USER/)
  } finally {
    getuidSpy.mockRestore()
  }
})
