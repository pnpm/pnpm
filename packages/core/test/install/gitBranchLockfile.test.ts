import fs from 'fs'
import path from 'path'
import { getCurrentBranch } from '@pnpm/git-utils/test/utils/mock'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { install, mutateModules } from '@pnpm/core'
import { testDefaults } from '../utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { ProjectManifest } from '@pnpm/types'
import writeYamlFile from 'write-yaml-file'

test('install with git-branch-lockfile = true', async () => {
  const project = prepareEmpty()

  const branchName: string = 'main-branch'
  getCurrentBranch.mockReturnValue(branchName)

  const opts = await testDefaults({
    useGitBranchLockfile: true,
  })

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, opts)

  const lockfileDir: string = project.dir()
  expect(fs.existsSync(path.join(lockfileDir, `pnpm-lock.${branchName}.yaml`))).toBe(true)
  expect(fs.existsSync(path.join(lockfileDir, WANTED_LOCKFILE))).toBe(false)
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
  const { root } = preparePackages([
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
  getCurrentBranch.mockReturnValue(branchName)

  const opts = await testDefaults({
    useGitBranchLockfile: true,
  })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: rootManifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: project1Manifest,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], opts)

  const lockfileDir: string = root.dir()
  expect(fs.existsSync(path.join(lockfileDir, `pnpm-lock.${branchName}.yaml`))).toBe(true)
  expect(fs.existsSync(path.join(lockfileDir, WANTED_LOCKFILE))).toBe(false)
})

test('install with --merge-git-branch-lockfiles', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  getCurrentBranch.mockReturnValue(branchName)

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  const mainLockfilePath: string = path.resolve(WANTED_LOCKFILE)
  await writeYamlFile(otherLockfilePath, {
    whatever: 'whatever',
  })

  expect(fs.existsSync(otherLockfilePath)).toBe(true)
  expect(fs.existsSync(mainLockfilePath)).toBe(false)

  const opts = await testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
  })
  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)
  expect(fs.existsSync(mainLockfilePath)).toBe(true)
})