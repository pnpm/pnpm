import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import isWindows from 'is-windows'

import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'lockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})

const testOnWindows = isWindows() ? test : test.skip

testOnWindows('fail when PNPM_HOME contains an unresolved Windows environment variable', () => {
  prepare()
  const result = execPnpmSync(['install'], {
    env: { PNPM_HOME: '%PNPM_TEST_UNSET_VARIABLE%\\pnpm' },
  })
  expect(result.status).not.toBe(0)
  const output = `${result.stdout?.toString()}${result.stderr?.toString()}`
  expect(output).toContain('PNPM_HOME contains an unexpanded environment variable: %PNPM_TEST_UNSET_VARIABLE%')
  expect(fs.existsSync('%PNPM_TEST_UNSET_VARIABLE%')).toBeFalsy()
})
