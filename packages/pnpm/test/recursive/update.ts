import { Lockfile } from '@pnpm/lockfile-types'
import { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import { addDistTag } from 'pnpm-registry-mock'
import readYamlFile from 'read-yaml-file'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('recursive update', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', 'is-positive@2.0.0')

  t.equal(projects['project-1'].requireModule('is-positive/package.json').version, '2.0.0')
  projects['project-2'].hasNot('is-positive')
})

// TODO: also cover the case of scoped package update
test('recursive update --latest foo should only update workspace packages that have foo', async (t: tape.Test) => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'qar', version: '100.0.0', distTag: 'latest' })

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'foo': '100.0.0',
        'qar': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '^100.0.0',
      },
    },
  ])

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  await execPnpm('recursive', 'install')

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm('recursive', 'update', '--latest', 'foo', 'qar@100.1.0')

  const lockfile = await readYamlFile<Lockfile>('./pnpm-lock.yaml')

  t.deepEqual(Object.keys(lockfile.packages || {}), ['/bar/100.0.0', '/foo/100.1.0', '/qar/100.1.0'])
})

test('recursive update --latest foo should only update packages that have foo', async (t: tape.Test) => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'qar', version: '100.0.0', distTag: 'latest' })

  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'foo': '100.0.0',
        'qar': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'bar': '^100.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm('recursive', 'update', '--latest', 'foo', 'qar@100.1.0')

  {
    const lockfile = await projects['project-1'].readLockfile()

    t.deepEqual(Object.keys(lockfile.packages || {}), ['/foo/100.1.0', '/qar/100.1.0'])
  }

  {
    const lockfile = await projects['project-2'].readLockfile()

    t.deepEqual(Object.keys(lockfile.packages || {}), ['/bar/100.0.0'])
  }
})
