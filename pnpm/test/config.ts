import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'useLockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})

test('read npmVersion from pnpm-workspace.yaml and download npm', async () => {
  const pnpmHome = path.resolve('pnpm_home')

  prepare({
    name: 'test-package',
    version: '1.0.0',
  })

  fs.writeFileSync('pnpm-workspace.yaml', 'npmVersion: "9.0.0"\npackages:\n  - "."\n', 'utf8')

  // Run publish with --dry-run to trigger npm installation
  // This will fail because npm path resolution has issues, but npm should be downloaded
  execPnpmSync(['publish', '--dry-run', '--no-git-checks'], {
    env: {
      PNPM_HOME: pnpmHome,
    },
  })

  const npmDir = path.join(pnpmHome, '.tools', 'npm', '9.0.0')
  expect(fs.existsSync(npmDir)).toBeTruthy()
}, 30000)
