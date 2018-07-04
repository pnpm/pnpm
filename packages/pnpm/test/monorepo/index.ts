import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {safeReadPackageFromDir} from '@pnpm/utils'
import {
  preparePackages,
  execPnpm,
 } from '../utils'

const test = promisifyTape(tape)

test('linking a package inside a monorepo', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {packages: ['**']})

  process.chdir('project-1')

  await execPnpm('link', 'project-2')

  const pkg = await safeReadPackageFromDir(process.cwd())
  t.deepEqual(pkg && pkg.dependencies, {'project-2': '^1.0.0'}, 'spec of linked package added to dependencies')
  await projects['project-1'].has('project-2')
})
