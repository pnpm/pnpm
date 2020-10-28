import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { Lockfile } from '@pnpm/lockfile-types'
import { install, update } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS } from '../utils'

test('recursive update', async () => {
  const projects = preparePackages(undefined, [
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive@2.0.0'])

  expect(projects['project-1'].requireModule('is-positive/package.json').version).toBe('2.0.0')
  await projects['project-2'].hasNot('is-positive')
})

test('recursive update prod dependencies only', async () => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })

  preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '^100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      devDependencies: {
        bar: '^100.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dev: false,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    optional: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const lockfile = await readYamlFile<Lockfile>('./pnpm-lock.yaml')
  expect(
    Object.keys(lockfile.packages ?? {})
  ).toStrictEqual(
    ['/bar/100.0.0', '/foo/100.1.0']
  )
})

test('recursive update with pattern', async () => {
  const projects = preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'peer-a': '1.0.0',
        'pnpm-foo': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'peer-c': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    latest: true,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['peer-*'])

  expect(projects['project-1'].requireModule('peer-a/package.json').version).toBe('1.0.1')
  expect(projects['project-1'].requireModule('pnpm-foo/package.json').version).toBe('1.0.0')
  expect(projects['project-2'].requireModule('peer-c/package.json').version).toBe('2.0.0')
})

test('recursive update with pattern and name in project', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'print-version', version: '2.0.0', distTag: 'latest' })

  const projects = preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'peer-a': '1.0.0',
        'pnpm-foo': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'peer-c': '1.0.0',
        'print-version': '1.0.0',
      },
    },
  ])

  const lockfileDir = process.cwd()

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  let err!: PnpmError
  try {
    await update.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      latest: true,
      lockfileDir,
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['this-does-not-exist'])
  } catch (_err) {
    err = _err
  }
  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES')

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    latest: true,
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['peer-*', 'print-version'])

  expect(projects['project-1'].requireModule('peer-a/package.json').version).toBe('1.0.1')
  expect(projects['project-1'].requireModule('pnpm-foo/package.json').version).toBe('1.0.0')
  expect(projects['project-2'].requireModule('peer-c/package.json').version).toBe('2.0.0')
  expect(projects['project-2'].requireModule('print-version/package.json').version).toBe('2.0.0')
})

test('recursive update --latest foo should only update projects that have foo', async () => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'qar', version: '100.0.0', distTag: 'latest' })

  preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
        qar: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@zkochan/async-regex-replace': '0.1.0',
        bar: '^100.0.0',
      },
    },
  ])

  const lockfileDir = process.cwd()

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    latest: true,
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['@zkochan/async-regex-replace', 'foo', 'qar@100.1.0'])

  const lockfile = await readYamlFile<Lockfile>('./pnpm-lock.yaml')

  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    '/@zkochan/async-regex-replace/0.2.0',
    '/bar/100.0.0',
    '/foo/100.1.0',
    '/qar/100.1.0',
  ])
})

test('recursive update --latest foo should only update packages that have foo', async () => {
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'qar', version: '100.0.0', distTag: 'latest' })

  const projects = preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        foo: '100.0.0',
        qar: '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        bar: '^100.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'bar', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    latest: true,
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['foo', 'qar@100.1.0'])

  {
    const lockfile = await projects['project-1'].readLockfile()

    expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['/foo/100.1.0', '/qar/100.1.0'])
  }

  {
    const lockfile = await projects['project-2'].readLockfile()

    expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['/bar/100.0.0'])
  }
})

test('recursive update in workspace should not add new dependencies', async () => {
  const projects = preparePackages(undefined, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await update.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    }, ['is-positive'])
  } catch (_err) {
    err = _err
  }
  expect(err).toBeTruthy()
  expect(err.code).toBe('ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES')

  await projects['project-1'].hasNot('is-positive')
  await projects['project-2'].hasNot('is-positive')
})
