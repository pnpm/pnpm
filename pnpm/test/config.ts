import fs from 'fs'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'lockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})
