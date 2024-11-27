import fs from 'fs'
import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { install, mutateModules } from '@pnpm/core'
import { testDefaults } from '../utils'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { type ProjectRootDir, type ProjectManifest } from '@pnpm/types'
import { getCurrentBranch } from '@pnpm/git-utils'
import { sync as writeYamlFile } from 'write-yaml-file'

jest.mock('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

test('install with git-branch-lockfile = true', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  ;(getCurrentBranch as jest.Mock).mockReturnValue(branchName)

  const opts = testDefaults({
    useGitBranchLockfile: true,
  })

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, opts)

  expect(fs.existsSync(`pnpm-lock.${branchName}.yaml`)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('install with git-branch-lockfile = true and no lockfile changes', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  ;(getCurrentBranch as jest.Mock).mockReturnValue(branchName)

  const manifest: ProjectManifest = {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }

  const opts1 = testDefaults({
    useGitBranchLockfile: false,
  })
  await install(manifest, opts1)

  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  const opts2 = testDefaults({
    useGitBranchLockfile: true,
  })
  await install(manifest, opts2)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)
  // Git branch lockfile is created only if there are changes in the lockfile
  expect(fs.existsSync(`pnpm-lock.${branchName}.yaml`)).toBe(false)
})

test('install a workspace with git-branch-lockfile = true', async () => {
  const rootManifest: ProjectManifest = {
    name: 'root',
  }
  const project1Manifest: ProjectManifest = {
    name: 'project-1',
    dependencies: { 'is-positive': '1.0.0' },
  }
  const project2Manifest: ProjectManifest = {
    name: 'project-2',
    dependencies: { 'is-positive': '1.0.0' },
  }
  preparePackages([
    {
      location: '.',
      package: rootManifest,
    },
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
  ])

  const branchName: string = 'main-branch'
  ;(getCurrentBranch as jest.Mock).mockReturnValue(branchName)

  const opts = testDefaults({
    useGitBranchLockfile: true,
    allProjects: [
      {
        buildIndex: 0,
        manifest: rootManifest,
        rootDir: process.cwd() as ProjectRootDir,
      },
      {
        buildIndex: 0,
        manifest: project1Manifest,
        rootDir: path.resolve('project-1') as ProjectRootDir,
      },
      {
        buildIndex: 0,
        manifest: project2Manifest,
        rootDir: path.resolve('project-2') as ProjectRootDir,
      },
    ],
  })

  await mutateModules([
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ], opts)

  expect(fs.existsSync(`pnpm-lock.${branchName}.yaml`)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('install with --merge-git-branch-lockfiles', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  ;(getCurrentBranch as jest.Mock).mockReturnValue(branchName)

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFile(otherLockfilePath, {
    whatever: 'whatever',
  })

  expect(fs.existsSync(otherLockfilePath)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)

  const opts = testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
  })
  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)
})

test('install with --merge-git-branch-lockfiles when merged lockfile is up to date', async () => {
  const project = prepareEmpty()

  // @types/semver installed in the main branch
  writeYamlFile(WANTED_LOCKFILE, {
    importers: {
      '.': {
        dependencies: {
          '@types/semver': {
            specifier: '5.3.31',
            version: '5.3.31',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@types/semver@5.3.31': {
        resolution: {
          integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
        },
      },
    },
    snapshots: {
      '@types/semver@5.3.31': {},
    },
  }, { lineWidth: 1000 })

  const branchName: string = 'main-branch'
  ;(getCurrentBranch as jest.Mock).mockReturnValue(branchName)

  // is-positive installed in the other branch
  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  const otherLockfileContent = {
    importers: {
      '.': {
        dependencies: {
          '@types/semver': {
            specifier: '5.3.31',
            version: '5.3.31',
          },
          'is-positive': {
            specifier: '^3.1.0',
            version: '3.1.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@types/semver@5.3.31': {
        resolution: {
          integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
        },
      },
      'is-positive@3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    snapshots: {
      '@types/semver@5.3.31': {},
      'is-positive@3.1.0': {},
    },
  }
  writeYamlFile(otherLockfilePath, otherLockfileContent, { lineWidth: 1000 })

  // the other branch merged to the main branch
  const projectManifest: ProjectManifest = {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-positive': '^3.1.0',
    },
  }
  const opts = testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    frozenLockfile: true,
  })
  await install(projectManifest, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(true)

  const wantedLockfileAfterMergeOther = project.readLockfile()
  expect(wantedLockfileAfterMergeOther).toEqual(otherLockfileContent)
})
