import fs from 'fs'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpmSync } from './utils/index.js'

test('read settings from pnpm-workspace.yaml', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'useLockfile: false', 'utf8')
  expect(execPnpmSync(['install']).status).toBe(0)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBeFalsy()
})

test('root resolutions are merged with workspace overrides', async () => {
  prepare({
    resolutions: {
      'is-odd': '3.0.1',
    },
  })
  fs.mkdirSync('packages/app', { recursive: true })
  fs.writeFileSync('packages/app/package.json', JSON.stringify({
    name: 'app',
    version: '1.0.0',
    dependencies: {
      'is-even': '^1.0.0',
    },
  }))
  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    overrides: {
      'is-number': '7.0.0',
      'is-odd': '3.0.1',
    },
  })

  expect(execPnpmSync(['install', '--lockfile-only']).status).toBe(0)

  const lockfile = readYamlFile<any>(WANTED_LOCKFILE) // eslint-disable-line
  expect(lockfile.overrides).toStrictEqual({
    'is-number': '7.0.0',
    'is-odd': '3.0.1',
  })
  expect(lockfile.packages).toHaveProperty(['is-number@7.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['is-number@6.0.0'])
})
