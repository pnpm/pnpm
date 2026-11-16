import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test.each(['recursive', 'filtered', 'explicit lockfile directory'])('fetch with dedicated lockfiles (%s)', (mode) => {
  const filtered = mode === 'filtered'
  const lockfileArgs = mode === 'explicit lockfile directory' ? ['--lockfile-dir', 'lockfiles'] : []
  prepareEmpty()
  fs.mkdirSync('lockfiles')
  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', private: true }))
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n')
  for (const [name, dependency] of [['a', 'is-positive'], ['b', 'is-negative']]) {
    fs.mkdirSync(`packages/${name}`, { recursive: true })
    fs.writeFileSync(`packages/${name}/package.json`, JSON.stringify({
      name,
      dependencies: { [dependency]: '1.0.0' },
    }))
  }
  execPnpmSync(['install', '--lockfile-only', ...lockfileArgs, '--store-dir', path.resolve('seed-store')], { expectSuccess: true })
  for (const name of ['a', 'b']) {
    expect(fs.existsSync(`packages/${name}/pnpm-lock.yaml`)).toBe(lockfileArgs.length === 0)
  }
  const selection = [...(filtered ? ['--filter', 'a'] : ['-r']), ...lockfileArgs]
  const modules = lockfileArgs.length ? 'lockfiles/node_modules' : 'packages/a/node_modules'
  const store = path.resolve('fetch-store')
  execPnpmSync([...selection, 'fetch', '--store-dir', store], { expectSuccess: true })
  expect(fs.existsSync(`${modules}/.pnpm/is-positive@1.0.0/node_modules/is-positive`)).toBe(true)
  expect(fs.existsSync(`${lockfileArgs.length ? 'lockfiles/node_modules' : 'packages/b/node_modules'}/.pnpm/is-negative@1.0.0/node_modules/is-negative`)).toBe(!filtered)
  expect(fs.existsSync(`${modules}/is-positive`)).toBe(false)
  execPnpmSync([...selection, 'install', '--offline', '--frozen-lockfile', '--store-dir', store], { expectSuccess: true })
})
