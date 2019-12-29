import { readWsPkgs } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { install, update } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import test = require('tape')
import { DEFAULT_OPTS } from './utils'

test('recursive update', async (t) => {
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

  const { allWsPkgs, selectedWsPkgsGraph } = await readWsPkgs(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  }, 'install')

  await update.handler(['is-positive@2.0.0'], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  })

  t.equal(projects['project-1'].requireModule('is-positive/package.json').version, '2.0.0')
  projects['project-2'].hasNot('is-positive')
  t.end()
})

test('recursive update --latest foo should only update workspace packages that have foo', async (t) => {
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
        '@zkochan/async-regex-replace': '0.1.0',
        'bar': '^100.0.0',
      },
    },
  ])

  const lockfileDir = process.cwd()

  const { allWsPkgs, selectedWsPkgsGraph } = await readWsPkgs(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  }, 'install')

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await update.handler(['@zkochan/async-regex-replace', 'foo', 'qar@100.1.0'], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    latest: true,
    lockfileDir,
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  })

  const lockfile = await readYamlFile<Lockfile>('./pnpm-lock.yaml')

  t.deepEqual(Object.keys(lockfile.packages || {}), ['/@zkochan/async-regex-replace/0.2.0', '/bar/100.0.0', '/foo/100.1.0', '/qar/100.1.0'])
  t.end()
})

test('recursive update --latest foo should only update packages that have foo', async (t) => {
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

  const { allWsPkgs, selectedWsPkgsGraph } = await readWsPkgs(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  }, 'install')

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await update.handler(['foo', 'qar@100.1.0'], {
    ...DEFAULT_OPTS,
    allWsPkgs,
    dir: process.cwd(),
    latest: true,
    recursive: true,
    selectedWsPkgsGraph,
    workspaceDir: process.cwd(),
  })

  {
    const lockfile = await projects['project-1'].readLockfile()

    t.deepEqual(Object.keys(lockfile.packages || {}), ['/foo/100.1.0', '/qar/100.1.0'])
  }

  {
    const lockfile = await projects['project-2'].readLockfile()

    t.deepEqual(Object.keys(lockfile.packages || {}), ['/bar/100.0.0'])
  }
  t.end()
})

test('recursive update in workspace should not add new dependencies', async (t) => {
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

  await update.handler(['is-positive'], {
    ...DEFAULT_OPTS,
    ...await readWsPkgs(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-positive')
  t.end()
})

test('recursive update should not add new dependencies', async (t) => {
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

  await update.handler(['is-positive'], {
    ...DEFAULT_OPTS,
    ...await readWsPkgs(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-positive')
  t.end()
})
