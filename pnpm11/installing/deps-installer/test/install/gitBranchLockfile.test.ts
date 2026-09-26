import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

import { testDefaults } from '../utils/index.js'

jest.unstable_mockModule('@pnpm/network.git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getCurrentBranch } = await import('@pnpm/network.git-utils')
const { install, mutateModules } = await import('@pnpm/installing.deps-installer')

test('install with git-branch-lockfile = true', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

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

test('git-branch-lockfile installs are not delegated to pacquet', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))
  const runPacquet = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({
    runPacquet: {
      supportsResolution: true,
      run: runPacquet,
    },
    useGitBranchLockfile: true,
  }))

  expect(runPacquet).not.toHaveBeenCalled()
  expect(fs.existsSync(`pnpm-lock.${branchName}.yaml`)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('install with git-branch-lockfile = true and no lockfile changes', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

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
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

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
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
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
  writeYamlFileSync(WANTED_LOCKFILE, {
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
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

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
  writeYamlFileSync(otherLockfilePath, otherLockfileContent, { lineWidth: 1000 })

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

test('--merge-git-branch-lockfiles keeps the branch lockfiles when lockfile handling is off', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
    importers: { '.': {} },
    lockfileVersion: LOCKFILE_VERSION,
  })

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    useLockfile: false,
  }))

  // Nothing read them, so nothing may delete them.
  expect(fs.existsSync(otherLockfilePath)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('--merge-git-branch-lockfiles keeps the branch lockfiles on a check-only install', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
    importers: { '.': {} },
    lockfileVersion: LOCKFILE_VERSION,
  })

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    dryRun: true,
  }))

  // The run only reports what it would do, so neither the merge it made
  // nor the deletion that would follow it may reach disk.
  expect(fs.existsSync(otherLockfilePath)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('--merge-git-branch-lockfiles keeps the branch lockfiles under lockfileCheck', async () => {
  prepareEmpty()

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
    importers: { '.': {} },
    lockfileVersion: LOCKFILE_VERSION,
  })

  // What `pnpm dedupe --check` runs: the lockfile is handed to the check
  // rather than kept, so the merge never survives the install.
  const lockfileCheck = jest.fn()
  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    lockfileCheck,
  }))
  expect(lockfileCheck).toHaveBeenCalled()
  expect(fs.existsSync(otherLockfilePath)).toBe(true)
  expect(fs.existsSync(WANTED_LOCKFILE)).toBe(false)
})

test('install with --merge-git-branch-lockfiles when a branch lockfile has a dependency that was removed', async () => {
  const project = prepareEmpty()

  // is-positive removed from the main branch, is-negative installed as a peer
  writeYamlFileSync(WANTED_LOCKFILE, {
    importers: {
      '.': {
        dependencies: {
          '@types/semver': {
            specifier: '5.3.31',
            version: '5.3.31',
          },
          'is-negative': {
            specifier: '^1.0.0',
            version: '1.0.0',
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
      'is-negative@1.0.0': {
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
    },
    snapshots: {
      '@types/semver@5.3.31': {},
      'is-negative@1.0.0': {},
    },
  }, { lineWidth: 1000 })

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  // the other branch was created before is-positive was removed
  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
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
  }, { lineWidth: 1000 })

  const projectManifest: ProjectManifest = {
    dependencies: {
      '@types/semver': '5.3.31',
    },
    peerDependencies: {
      'is-negative': '^1.0.0',
    },
  }
  const opts = testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    frozenLockfile: true,
  })
  await install(projectManifest, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)

  const wantedLockfileAfterMergeOther = project.readLockfile()
  expect(wantedLockfileAfterMergeOther.importers['.'].dependencies).toStrictEqual({
    '@types/semver': {
      specifier: '5.3.31',
      version: '5.3.31',
    },
    'is-negative': {
      specifier: '^1.0.0',
      version: '1.0.0',
    },
  })
  expect(wantedLockfileAfterMergeOther.packages).not.toHaveProperty(['is-positive@3.1.0'])
  expect(wantedLockfileAfterMergeOther.snapshots).not.toHaveProperty(['is-positive@3.1.0'])
  project.hasNot('is-positive')
})

test('install with --merge-git-branch-lockfiles when a branch lockfile has a dependency in another group', async () => {
  const project = prepareEmpty()

  // @types/semver moved to dependencies in the main branch
  writeYamlFileSync(WANTED_LOCKFILE, {
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
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  // the other branch still has it in devDependencies
  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
    importers: {
      '.': {
        devDependencies: {
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

  const projectManifest: ProjectManifest = {
    dependencies: {
      '@types/semver': '5.3.31',
    },
  }
  const opts = testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    frozenLockfile: true,
  })
  await install(projectManifest, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)

  const wantedLockfileAfterMergeOther = project.readLockfile()
  expect(wantedLockfileAfterMergeOther.importers['.'].devDependencies).toBeUndefined()
})

test('install with --merge-git-branch-lockfiles keeps a dependency that is also declared as a peer', async () => {
  const project = prepareEmpty()

  writeYamlFileSync(WANTED_LOCKFILE, {
    importers: {
      '.': {
        devDependencies: {
          'is-negative': {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-negative@1.0.0': {
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
    },
    snapshots: {
      'is-negative@1.0.0': {},
    },
  }, { lineWidth: 1000 })

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  // the other branch was created before is-positive was removed
  const otherLockfilePath: string = path.resolve('pnpm-lock.other.yaml')
  writeYamlFileSync(otherLockfilePath, {
    importers: {
      '.': {
        dependencies: {
          'is-positive': {
            specifier: '^3.1.0',
            version: '3.1.0',
          },
        },
        devDependencies: {
          'is-negative': {
            specifier: '^1.0.0',
            version: '1.0.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-negative@1.0.0': {
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
      'is-positive@3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    snapshots: {
      'is-negative@1.0.0': {},
      'is-positive@3.1.0': {},
    },
  }, { lineWidth: 1000 })

  // A peer that another field already declares is not auto-installed, so it
  // stays under that field rather than moving to `dependencies`.
  const projectManifest: ProjectManifest = {
    devDependencies: {
      'is-negative': '^1.0.0',
    },
    peerDependencies: {
      'is-negative': '^1.0.0',
    },
  }
  const opts = testDefaults({
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
    frozenLockfile: true,
  })
  await install(projectManifest, opts)

  expect(fs.existsSync(otherLockfilePath)).toBe(false)

  const wantedLockfileAfterMergeOther = project.readLockfile()
  expect(wantedLockfileAfterMergeOther.importers['.'].devDependencies).toStrictEqual({
    'is-negative': {
      specifier: '^1.0.0',
      version: '1.0.0',
    },
  })
  expect(wantedLockfileAfterMergeOther.importers['.'].dependencies).toBeUndefined()
  project.hasNot('is-positive')
})

test.each([
  ['no branch lockfile exists', undefined],
  ['the only branch lockfile is empty', ''],
  ['the only branch lockfile has no lockfile document', 'lockfileVersion: \'9.0\'\n'],
])('--merge-git-branch-lockfiles still rejects an outdated lockfile when %s', async (_name, branchLockfileContent) => {
  prepareEmpty()

  // The lockfile records a dependency the manifest never declared, and no
  // branch lockfile exists to explain it.
  writeYamlFileSync(WANTED_LOCKFILE, {
    importers: {
      '.': {
        dependencies: {
          'is-positive': {
            specifier: '^3.1.0',
            version: '3.1.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-positive@3.1.0': {
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    snapshots: {
      'is-positive@3.1.0': {},
    },
  }, { lineWidth: 1000 })

  const branchName: string = 'main-branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))

  if (branchLockfileContent != null) {
    fs.writeFileSync(path.resolve('pnpm-lock.other.yaml'), branchLockfileContent)
  }

  await expect(
    install({}, testDefaults({
      useGitBranchLockfile: true,
      mergeGitBranchLockfiles: true,
      frozenLockfile: true,
    }))
  ).rejects.toThrow(/ERR_PNPM_OUTDATED_LOCKFILE|not up to date/)
})
