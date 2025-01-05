import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type LockfileFile } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import { createPeersDirSuffix } from '@pnpm/dependency-path'
import { sync as loadJsonFile } from 'load-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'

test('deduplicate packages that have peers, when adding new dependency in a workspace', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-grand-parent-with-c': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', `dedupe-peer-dependents=true
auto-install-peers=false`, 'utf8')
  await execPnpm(['install'])
  await execPnpm(['--filter=project-2', 'add', '@pnpm.e2e/abc@1.0.0'])

  const lockfile = readYamlFile<LockfileFile>(path.resolve(WANTED_LOCKFILE))
  const depPaths = Object.keys(lockfile.snapshots ?? {})
  expect(depPaths.length).toBe(8)
  expect(depPaths).toContain(`@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
  expect(depPaths).toContain(`@pnpm.e2e/abc-parent-with-ab@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
})

test('partial update in a workspace should work with dedupe-peer-dependents is true', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/abc', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-grand-parent-with-c': '^1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',

        dependencies: {
          '@pnpm.e2e/abc-grand-parent-with-c': '^1.0.0',
        },
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', `dedupe-peer-dependents=true
auto-install-peers=false`, 'utf8')
  await execPnpm(['install'])
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  process.chdir('project-2')
  await execPnpm(['update'])
  process.chdir('..')

  expect(loadJsonFile<any>('project-1/package.json').dependencies['@pnpm.e2e/abc-grand-parent-with-c']).toBe('^1.0.0') // eslint-disable-line
  expect(loadJsonFile<any>('project-2/package.json').dependencies['@pnpm.e2e/abc-grand-parent-with-c']).toBe('^1.0.1') // eslint-disable-line
})

// Covers https://github.com/pnpm/pnpm/issues/8877
test('partial update --latest in a workspace should not affect other packages when dedupe-peer-dependents is true', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/foo': '1.0.0',
          '@pnpm.e2e/bar': '100.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',

        dependencies: {
          '@pnpm.e2e/foo': '1.0.0',
        },
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', `dedupe-peer-dependents=true
auto-install-peers=false`, 'utf8')
  await execPnpm(['install'])

  await addDistTag({ package: '@pnpm.e2e/foo', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await execPnpm(['update', '--filter', 'project-2', '--latest'])

  // project 1's manifest is unaffected, while project 2 has foo updated
  expect(loadJsonFile<any>('project-1/package.json').dependencies['@pnpm.e2e/foo']).toBe('1.0.0') // eslint-disable-line
  expect(loadJsonFile<any>('project-1/package.json').dependencies['@pnpm.e2e/bar']).toBe('100.0.0') // eslint-disable-line
  expect(loadJsonFile<any>('project-2/package.json').dependencies['@pnpm.e2e/foo']).toBe('2.0.0') // eslint-disable-line

  // similar for the importers in the lockfile; project 1 is unaffected, while
  // project 2 resolves the latest foo
  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.importers['project-1']?.dependencies?.['@pnpm.e2e/foo'].version).toStrictEqual('1.0.0')
  expect(lockfile.importers['project-1']?.dependencies?.['@pnpm.e2e/bar'].version).toStrictEqual('100.0.0')
  expect(lockfile.importers['project-2']?.dependencies?.['@pnpm.e2e/foo'].version).toStrictEqual('2.0.0')
})

// Covers https://github.com/pnpm/pnpm/issues/6154
test('peer dependents deduplication should not remove peer dependencies', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  preparePackages([
    {
      location: '',
      package: {
        name: 'project-1',

        dependencies: {
          '@pnpm.e2e/abc-parent-with-missing-peers': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', `dedupe-peer-dependents=true
auto-install-peers=true`, 'utf8')
  await execPnpm(['install'])
  await execPnpm(['--filter=project-2', 'add', 'is-positive@1.0.0'])

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.importers['.']?.dependencies?.['@pnpm.e2e/abc-parent-with-missing-peers'].version).toStrictEqual('1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)')
})
