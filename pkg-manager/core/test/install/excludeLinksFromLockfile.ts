import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  type MutatedProject,
  type ProjectOptions,
} from '@pnpm/core'
import { type Lockfile, type LockfileFile } from '@pnpm/lockfile-types'
import { type ProjectRootDir, type ProjectId } from '@pnpm/types'
import { prepareEmpty, preparePackages, tempDir } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as rimraf } from '@zkochan/rimraf'
import normalizePath from 'normalize-path'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeJsonFile } from 'write-json-file'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('links are not added to the lockfile when excludeLinksFromLockfile is true', async () => {
  const externalPkg1 = tempDir(false)
  fs.writeFileSync(path.join(externalPkg1, 'index.js'), '', 'utf8')
  const externalPkg2 = tempDir(false)
  fs.writeFileSync(path.join(externalPkg2, 'index.js'), '', 'utf8')
  const externalPkg3 = tempDir(false)
  fs.writeFileSync(path.join(externalPkg3, 'index.js'), '', 'utf8')
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const project1Dir = path.resolve('project-1')
  const project2Dir = path.resolve('project-2')
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'external-1': `link:${externalPkg1}`,
        },
      },
      rootDir: project1Dir as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
          'external-2': `link:${path.relative(project2Dir, externalPkg2)}`,
        },
      },
      rootDir: project2Dir as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true }))
  const lockfile: LockfileFile = readYamlFile(WANTED_LOCKFILE)
  expect(lockfile.importers?.['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers?.['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1/index.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2/index.js'))).toBeTruthy()

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')

  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true, frozenLockfile: true }))
  expect(lockfile.importers?.['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers?.['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1/index.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2/index.js'))).toBeTruthy()

  rimraf('node_modules')
  rimraf('project-1/node_modules')
  rimraf('project-2/node_modules')

  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true, frozenLockfile: false, preferFrozenLockfile: false }))
  expect(lockfile.importers?.['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers?.['project-2'].dependencies?.['external-2']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1/index.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-2/index.js'))).toBeTruthy()

  delete allProjects[1].manifest.dependencies!['external-2']
  allProjects[1].manifest.dependencies!['external-3'] = `link:${path.relative(project2Dir, externalPkg3)}`
  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true }))
  expect(lockfile.importers?.['project-1'].dependencies?.['external-1']).toBeUndefined()
  expect(lockfile.importers?.['project-2'].dependencies?.['external-2']).toBeUndefined()
  expect(lockfile.importers?.['project-2'].dependencies?.['external-3']).toBeUndefined()

  expect(fs.existsSync(path.resolve('project-1/node_modules/external-1/index.js'))).toBeTruthy()
  // expect(fs.existsSync(path.resolve('project-2/node_modules/external-2'))).toBeFalsy() // Should we remove external links that are not in deps anymore?
  expect(fs.existsSync(path.resolve('project-2/node_modules/external-3/index.js'))).toBeTruthy()
})

test('local file using absolute path is correctly installed on repeat install', async () => {
  const project = prepareEmpty()
  const absolutePath = path.resolve('..', 'local-pkg')
  f.copy('local-pkg', absolutePath)

  // is-odd is only added because otherwise no lockfile is created
  const manifest = await addDependenciesToPackage({},
    [`link:${absolutePath}`, 'is-odd@1.0.0'],
    testDefaults({ excludeLinksFromLockfile: true })
  )

  const expectedSpecs = {
    'is-odd': '1.0.0',
    'local-pkg': `link:${normalizePath(absolutePath)}`,
  }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true, excludeLinksFromLockfile: true }))
  {
    const m = project.requireModule('local-pkg')
    expect(m).toBeTruthy()
  }
})

test('hoisted install should not fail with excludeLinksFromLockfile true', async () => {
  const project = prepareEmpty()
  const absolutePath = path.resolve('..', 'local-pkg')
  f.copy('local-pkg', absolutePath)

  // is-odd is only added because otherwise no lockfile is created
  const manifest = await addDependenciesToPackage({},
    [`link:${absolutePath}`, 'is-odd@1.0.0'],
    testDefaults({ excludeLinksFromLockfile: true, nodeLinker: 'hoisted' })
  )

  const expectedSpecs = {
    'is-odd': '1.0.0',
    'local-pkg': `link:${normalizePath(absolutePath)}`,
  }
  expect(manifest.dependencies).toStrictEqual(expectedSpecs)

  const m = project.requireModule('local-pkg')
  expect(m).toBeTruthy()
})

test('update the lockfile when a new project is added to the workspace but do not add external links', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])
  const absolutePath = path.resolve('..', 'local-pkg')
  f.copy('local-pkg', absolutePath)

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'local-pkg': `link:${normalizePath(absolutePath)}`,
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true }))

  importers.push({
    mutation: 'install',
    rootDir: path.resolve('project-2') as ProjectRootDir,
  })
  allProjects.push({
    buildIndex: 0,
    manifest: {
      name: 'project-2',
      version: '1.0.0',
    },
    rootDir: path.resolve('project-2') as ProjectRootDir,
  })
  await mutateModules(importers, testDefaults({ allProjects, excludeLinksFromLockfile: true, frozenLockfile: true }))

  const lockfile: Lockfile = readYamlFile(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1', 'project-2'])
  expect(Object.keys(lockfile.importers['project-1' as ProjectId].dependencies ?? {})).toStrictEqual(['is-positive'])
})

test('path to external link is not added to the lockfile, when it resolves a peer dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  const externalPkg = tempDir(false)
  writeJsonFile(path.join(externalPkg, 'package.json'), {
    name: '@pnpm.e2e/peer-a',
    version: '1.0.0',
  })
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    ['@pnpm.e2e/abc@1.0.0', `link:${externalPkg}`],
    testDefaults({ excludeLinksFromLockfile: true })
  )

  const lockfile = project.readLockfile()
  const key = '@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@node_modules+@pnpm.e2e+peer-a)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)'
  expect(lockfile.snapshots[key]).toBeTruthy()
  expect(lockfile.snapshots[key].dependencies?.['@pnpm.e2e/peer-a']).toBe('link:node_modules/@pnpm.e2e/peer-a')
})

test('links resolved from workspace protocol dependencies are not removed', async () => {
  const pkg1 = {
    name: 'project-1',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
      'project-2': 'workspace:*',
    },
  }
  const pkg2 = {
    name: 'project-2',
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  preparePackages([pkg1, pkg2])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: pkg1,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: pkg2,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    excludeLinksFromLockfile: true,
    lockfileOnly: true,
  }))

  const lockfile: LockfileFile = readYamlFile(WANTED_LOCKFILE)
  expect(lockfile.importers?.['project-1'].dependencies?.['project-2']).toStrictEqual({
    specifier: 'workspace:*',
    version: 'link:../project-2',
  })
})
