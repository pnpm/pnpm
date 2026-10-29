import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { findDanglingDirectDependencyLink, type FindDanglingDirectDependencyLinkOptions } from '@pnpm/deps.status'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

let root: string

beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'dangling-link-'))
  fs.mkdirSync(path.join(root, 'lib'))
  fs.mkdirSync(path.join(root, 'other'))
})

afterEach(() => {
  fs.rmSync(root, { recursive: true, force: true })
})

const rootManifest: ProjectManifest = { name: 'root', version: '1.0.0', dependencies: { lib: 'workspace:*' } }
const appManifest: ProjectManifest = { name: 'app', version: '1.0.0', dependencies: { lib: 'workspace:*' } }

function writeLockfile (appLibVersion: string | undefined): void {
  const appDependencies = appLibVersion == null
    ? '  app: {}\n'
    : `  app:\n    dependencies:\n      lib:\n        specifier: workspace:*\n        version: ${appLibVersion}\n`
  fs.writeFileSync(path.join(root, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      lib:
        specifier: workspace:*
        version: link:lib

${appDependencies}`)
}

function link (target: string, entry: string): void {
  fs.mkdirSync(path.dirname(path.join(root, entry)), { recursive: true })
  fs.symlinkSync(path.join(root, target), path.join(root, entry), 'junction')
}

async function findDangling (opts: Partial<FindDanglingDirectDependencyLinkOptions> = {}): Promise<string | undefined> {
  return findDanglingDirectDependencyLink({
    lockfileDir: root,
    modulesDir: 'node_modules',
    nodeLinker: 'isolated',
    rootProjectManifestDir: root,
    rootProjectManifest: rootManifest,
    allProjects: [
      { rootDir: root as ProjectRootDir, manifest: rootManifest },
      { rootDir: path.join(root, 'app') as ProjectRootDir, manifest: appManifest },
    ] as FindDanglingDirectDependencyLinkOptions['allProjects'],
    ...opts,
  })
}

test('a deleted workspace link the lockfile records is reported', async () => {
  writeLockfile('link:../lib')
  link('lib', 'node_modules/lib')
  fs.mkdirSync(path.join(root, 'app/node_modules'), { recursive: true })

  expect(await findDangling()).toBe(path.join(root, 'app/node_modules/lib'))
})

test('a workspace dependency the lockfile does not link may be missing', async () => {
  link('lib', 'node_modules/lib')
  fs.mkdirSync(path.join(root, 'app/node_modules'), { recursive: true })

  writeLockfile(undefined)
  expect(await findDangling()).toBeUndefined()

  writeLockfile('file:lib')
  expect(await findDangling()).toBeUndefined()
})

test('a root link to the same target stands in for a deduped one, under any alias', async () => {
  writeLockfile('link:../lib')
  link('lib', 'node_modules/renamed')
  fs.mkdirSync(path.join(root, 'app/node_modules'), { recursive: true })

  const renamingRoot: ProjectManifest = { name: 'root', dependencies: { renamed: 'workspace:lib@*' } }
  expect(await findDangling({
    dedupeDirectDeps: true,
    rootProjectManifest: renamingRoot,
    allProjects: [
      { rootDir: root as ProjectRootDir, manifest: renamingRoot },
      { rootDir: path.join(root, 'app') as ProjectRootDir, manifest: appManifest },
    ] as FindDanglingDirectDependencyLinkOptions['allProjects'],
  })).toBeUndefined()
})

test('a root link to another target does not stand in for a deduped one', async () => {
  writeLockfile('link:../other')
  link('lib', 'node_modules/lib')
  fs.mkdirSync(path.join(root, 'app/node_modules'), { recursive: true })

  expect(await findDangling({ dedupeDirectDeps: true })).toBe(path.join(root, 'app/node_modules/lib'))
})

test('a missing workspace link is ignored when symlinks are off or the project is not selected', async () => {
  writeLockfile('link:../lib')
  link('lib', 'node_modules/lib')
  fs.mkdirSync(path.join(root, 'app/node_modules'), { recursive: true })

  expect(await findDangling({ symlink: false })).toBeUndefined()
  expect(await findDangling({ selectedProjectsGraph: { [root]: {} } as unknown as FindDanglingDirectDependencyLinkOptions['selectedProjectsGraph'] })).toBeUndefined()
})
