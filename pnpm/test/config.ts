import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from './utils'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'useLockfile: false', 'utf8')
  const { status } = execPnpmSync(['install'], {
    env: {
      PNPM_HOME: path.resolve('pnpm_home'),
    },
  })
  expect(status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})
