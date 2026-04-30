import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

// Covers https://github.com/pnpm/pnpm/issues/11403
//
// `pnpm add -g` running over a package that needs an approve-builds prompt
// used to forward an absolute `modulesDir` (`<installDir>/node_modules`)
// into the install run by `approve-builds`. The install layer treated
// `modulesDir` as a path relative to `lockfileDir` and joined it again,
// producing a doubled prefix on Windows because `path.join` does not
// collapse an embedded absolute path. The hoist step then failed with
// `ENOENT` while trying to symlink under `<installDir>\<installDir>\...`.
//
// We exercise the same broken code path via the CLI by passing an
// absolute `--modules-dir` directly. Before the fix this crashed with
// the doubled-prefix `ENOENT`. After the fix the install succeeds and
// the hoisted dep lands in `<modulesDir>/.pnpm/node_modules`.
test('pnpm install accepts an absolute --modules-dir', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const absoluteModulesDir = path.resolve('node_modules')

  await execPnpm([
    'install',
    `--modules-dir=${absoluteModulesDir}`,
    '--config.hoist-pattern=*',
  ])

  expect(fs.existsSync(path.join(absoluteModulesDir, '@pnpm.e2e/pkg-with-1-dep/package.json'))).toBe(true)
  expect(fs.existsSync(path.join(absoluteModulesDir, '.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBe(true)

  // The frozen-lockfile path goes through `headlessInstall` and is the
  // exact code path that produced the doubled `ENOENT` in the bug report.
  fs.rmSync(absoluteModulesDir, { recursive: true })
  await execPnpm([
    'install',
    `--modules-dir=${absoluteModulesDir}`,
    '--config.hoist-pattern=*',
    '--frozen-lockfile',
  ])

  expect(fs.existsSync(path.join(absoluteModulesDir, '@pnpm.e2e/pkg-with-1-dep/package.json'))).toBe(true)
  expect(fs.existsSync(path.join(absoluteModulesDir, '.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBe(true)
})
