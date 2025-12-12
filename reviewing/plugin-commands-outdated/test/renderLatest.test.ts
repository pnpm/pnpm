import { outdated } from '@pnpm/plugin-commands-outdated'
import semverDiff from '@pnpm/semver-diff'
import { type PackageManifest } from '@pnpm/types'
import { type OutdatedWithVersionDiff } from '../src/utils.js'
import chalk from 'chalk'

test('renderLatest: outdated and deprecated', () => {
  const diffResult = semverDiff.default('0.0.1', '1.0.0')
  const outdatedPkg: OutdatedWithVersionDiff = {
    ...diffResult,
    alias: 'foo',
    belongsTo: 'dependencies',
    current: '0.0.1',
    latestManifest: {
      name: 'foo',
      version: '1.0.0',
      deprecated: 'This package is deprecated',
    } as PackageManifest,
    packageName: 'foo',
    wanted: '0.0.1',
  }

  const output = outdated.renderLatest(outdatedPkg)

  expect(output).toContain('(deprecated)')
  expect(output).toContain('1.0.0')
  expect(output).toContain(chalk.redBright('(deprecated)'))
})
