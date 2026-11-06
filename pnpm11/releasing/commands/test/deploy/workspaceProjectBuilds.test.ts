import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

import { describe, expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import type { PackageSnapshots } from '@pnpm/lockfile.types'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { deploy } from '@pnpm/releasing.commands'
import type { DepPath, ProjectManifest, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { writeYamlFile } from 'write-yaml-file'

import {
  approveLegacyWorkspaceProjectBuilds,
  approveWorkspaceProjectBuilds,
} from '../../src/deploy/workspaceProjectBuilds.js'
import { DEFAULT_OPTS } from './utils/index.js'

const BUILD_SCRIPT = 'node -e "require(\'fs\').writeFileSync(\'built.txt\', \'1\')"'

async function prepareWorkspaceWithBuiltLib () {
  const opts = await prepareWorkspace({ name: 'lib', version: '1.0.0', scripts: { postinstall: BUILD_SCRIPT } })
  // The source install runs the project's build, and the deploy copies the
  // project's files, so the output is removed to observe the deploy's build.
  fs.rmSync('lib/built.txt')
  return opts
}

/**
 * Installs a workspace where `app` depends on `lib`. `libFiles` are written
 * under `lib` before the install.
 */
async function prepareWorkspace (libManifest: ProjectManifest, libFiles: Record<string, string> = {}) {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0', private: true } },
    { name: 'app', version: '1.0.0', dependencies: { lib: 'workspace:*' } },
    libManifest,
  ])
  for (const [file, content] of Object.entries(libFiles)) {
    fs.mkdirSync(path.dirname(path.join('lib', file)), { recursive: true })
    fs.writeFileSync(path.join('lib', file), content)
  }
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['app', 'lib'] })
  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'app' }])
  const workspaceOpts = {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
    sharedWorkspaceLockfile: true,
    workspaceDir: process.cwd(),
  }
  await install.handler({
    ...workspaceOpts,
    allProjectsGraph,
    injectWorkspacePackages: false,
    recursive: true,
    selectedProjectsGraph: allProjectsGraph,
  })
  return { ...workspaceOpts, selectedProjectsGraph }
}

test.each([
  ['node-linker=isolated', { nodeLinker: 'isolated' as const }],
  ['node-linker=hoisted', { nodeLinker: 'hoisted' as const }],
  ['the legacy implementation', { forceLegacyDeploy: true }],
])('deploy with %s builds a workspace project it converts to a file: dependency', async (_, deployOpts) => {
  const opts = await prepareWorkspaceWithBuiltLib()

  await deploy.handler({ ...opts, ...deployOpts }, ['deploy'])

  expect(fs.existsSync('deploy/node_modules/lib/built.txt')).toBe(true)
})

test.each([
  ['node-linker=isolated', { nodeLinker: 'isolated' as const }],
  ['node-linker=hoisted', { nodeLinker: 'hoisted' as const }],
  ['the legacy implementation', { forceLegacyDeploy: true }],
])('deploy with %s builds the publish directory of a workspace project', async (_, deployOpts) => {
  const opts = await prepareWorkspace(
    { name: 'lib', version: '1.0.0', publishConfig: { directory: 'dist' } },
    { 'dist/package.json': JSON.stringify({ name: 'lib', version: '1.0.0', scripts: { postinstall: BUILD_SCRIPT } }) }
  )

  await deploy.handler({ ...opts, ...deployOpts }, ['deploy'])

  expect(fs.existsSync('deploy/node_modules/lib/built.txt')).toBe(true)
})

test('deploy keeps an explicit denial of a workspace project build', async () => {
  const opts = await prepareWorkspaceWithBuiltLib()

  await deploy.handler({ ...opts, allowBuilds: { lib: false } }, ['deploy'])

  expect(fs.existsSync('deploy/node_modules/lib/built.txt')).toBe(false)
})

const workspaceDir = path.resolve('workspace')
const deployDir = path.join(workspaceDir, 'deploy')
const libDir = path.join(workspaceDir, 'lib')
const allProjects = [{
  rootDir: libDir as ProjectRootDir,
  rootDirRealPath: libDir as ProjectRootDirRealPath,
  manifest: { name: 'lib', version: '1.0.0' },
}]

function fileKey (dir: string): DepPath {
  return `${path.basename(dir)}@${url.pathToFileURL(dir).toString()}` as DepPath
}

function directoryResolution (dir: string): PackageSnapshots[DepPath] {
  return { resolution: { type: 'directory', directory: path.relative(deployDir, dir) } }
}

test('approveWorkspaceProjectBuilds approves a workspace project by its dependency path', () => {
  const packageSnapshots: PackageSnapshots = {
    [fileKey(libDir)]: directoryResolution(libDir),
    [`${fileKey(libDir)}(peer@1.0.0)` as DepPath]: directoryResolution(libDir),
  }

  expect(approveWorkspaceProjectBuilds({ lib: true }, { allProjects, deployDir, lockfileDir: workspaceDir, packageSnapshots }))
    .toStrictEqual({ lib: true, [fileKey(libDir)]: true })
})

test('approveWorkspaceProjectBuilds does not approve a directory that is not a workspace project', () => {
  const vendoredDir = path.join(workspaceDir, 'vendor/lib')
  const packageSnapshots: PackageSnapshots = {
    [fileKey(vendoredDir)]: directoryResolution(vendoredDir),
  }

  expect(approveWorkspaceProjectBuilds(undefined, { allProjects, deployDir, lockfileDir: workspaceDir, packageSnapshots })).toBeUndefined()
})

test('approveWorkspaceProjectBuilds does not approve a project key that one of its snapshots redirects', () => {
  const packageSnapshots: PackageSnapshots = {
    [fileKey(libDir)]: directoryResolution(libDir),
    [`${fileKey(libDir)}(peer@1.0.0)` as DepPath]: directoryResolution(path.join(workspaceDir, 'vendor/lib')),
  }

  expect(approveWorkspaceProjectBuilds(undefined, { allProjects, deployDir, lockfileDir: workspaceDir, packageSnapshots })).toBeUndefined()
})

test('approveWorkspaceProjectBuilds keeps a denial of a workspace project', () => {
  const packageSnapshots: PackageSnapshots = {
    [fileKey(libDir)]: directoryResolution(libDir),
  }

  expect(approveWorkspaceProjectBuilds({ lib: false }, { allProjects, deployDir, lockfileDir: workspaceDir, packageSnapshots }))
    .toStrictEqual({ lib: false })
})

test('approveWorkspaceProjectBuilds keeps a denial of the key the source workspace gives a project', () => {
  const packageSnapshots: PackageSnapshots = {
    [fileKey(libDir)]: directoryResolution(libDir),
  }

  expect(approveWorkspaceProjectBuilds(
    { 'lib@file:lib': false },
    { allProjects, deployDir, lockfileDir: workspaceDir, packageSnapshots }
  )).toStrictEqual({ 'lib@file:lib': false })
})

test('approveLegacyWorkspaceProjectBuilds approves a workspace project relative to the lockfile dir', () => {
  const deniedDir = path.join(workspaceDir, 'denied')
  const projects = [...allProjects, {
    rootDir: deniedDir as ProjectRootDir,
    rootDirRealPath: deniedDir as ProjectRootDirRealPath,
    manifest: { name: 'denied', version: '1.0.0' },
  }]

  expect(approveLegacyWorkspaceProjectBuilds({ denied: false }, { allProjects: projects, lockfileDir: workspaceDir }))
    .toStrictEqual({ denied: false, 'lib@file:lib': true })
})

test('approveLegacyWorkspaceProjectBuilds keeps a denial of either root path of a project', () => {
  const realDir = path.join(workspaceDir, 'real/lib')
  const projects = [{ ...allProjects[0], rootDirRealPath: realDir as ProjectRootDirRealPath }]

  expect(approveLegacyWorkspaceProjectBuilds({ 'lib@file:real/lib': false }, { allProjects: projects, lockfileDir: workspaceDir }))
    .toStrictEqual({ 'lib@file:real/lib': false })
})

describe('the publish directory of a workspace project', () => {
  function projectWithPublishDir (rootDir: string, directory: string) {
    return {
      rootDir: rootDir as ProjectRootDir,
      rootDirRealPath: rootDir as ProjectRootDirRealPath,
      manifest: { name: 'lib', version: '1.0.0', publishConfig: { directory } },
    }
  }

  function approvePublishDir (directory: string, packageDir: string, allowBuilds?: Record<string, boolean>) {
    const root = fs.realpathSync(tempDir())
    const projectDir = path.join(root, 'lib')
    fs.mkdirSync(path.join(projectDir, packageDir), { recursive: true })
    const packageSnapshots: PackageSnapshots = {
      [`lib@${url.pathToFileURL(path.join(projectDir, packageDir)).toString()}` as DepPath]: {
        resolution: { type: 'directory', directory: path.relative(path.join(root, 'deploy'), path.join(projectDir, packageDir)) },
      },
    }
    return approveWorkspaceProjectBuilds(allowBuilds, {
      allProjects: [projectWithPublishDir(projectDir, directory)],
      deployDir: path.join(root, 'deploy'),
      lockfileDir: root,
      packageSnapshots,
    })
  }

  test('is approved by its dependency path', () => {
    expect(Object.keys(approvePublishDir('dist', 'dist') ?? {})).toHaveLength(1)
  })

  test('is not approved outside the project root', () => {
    expect(approvePublishDir('../vendor', '../vendor')).toBeUndefined()
  })

  test('is approved under a name that starts with two dots', () => {
    expect(Object.keys(approvePublishDir('..dist', '..dist') ?? {})).toHaveLength(1)
  })

  test('is not approved inside node_modules', () => {
    expect(approvePublishDir('node_modules/dep', 'node_modules/dep')).toBeUndefined()
  })

  test('is not approved inside node_modules spelled in another case', () => {
    expect(approvePublishDir('NODE_MODULES/dep', 'NODE_MODULES/dep')).toBeUndefined()
  })

  test('is not approved when the source workspace denies the project root', () => {
    expect(approvePublishDir('dist', 'dist', { 'lib@file:lib': false })).toStrictEqual({ 'lib@file:lib': false })
  })

  test('is not approved through a symlink', () => {
    const root = fs.realpathSync(tempDir())
    const projectDir = path.join(root, 'lib')
    fs.mkdirSync(path.join(root, 'vendor'), { recursive: true })
    fs.mkdirSync(projectDir)
    fs.symlinkSync(path.join(root, 'vendor'), path.join(projectDir, 'dist'), 'junction')
    const publishDir = path.join(projectDir, 'dist')
    const packageSnapshots: PackageSnapshots = {
      [`lib@${url.pathToFileURL(publishDir).toString()}` as DepPath]: {
        resolution: { type: 'directory', directory: path.relative(path.join(root, 'deploy'), publishDir) },
      },
    }

    expect(approveWorkspaceProjectBuilds(undefined, {
      allProjects: [projectWithPublishDir(projectDir, 'dist')],
      deployDir: path.join(root, 'deploy'),
      lockfileDir: root,
      packageSnapshots,
    })).toBeUndefined()
  })

  test('is approved under its source workspace key by a legacy deploy', () => {
    const root = fs.realpathSync(tempDir())
    const projectDir = path.join(root, 'lib')
    fs.mkdirSync(path.join(projectDir, 'dist'), { recursive: true })

    expect(approveLegacyWorkspaceProjectBuilds(undefined, {
      allProjects: [projectWithPublishDir(projectDir, 'dist')],
      lockfileDir: root,
    })).toStrictEqual({ 'lib@file:lib': true, 'lib@file:lib/dist': true })
  })

  test('denied under its source workspace key blocks the project root of a legacy deploy', () => {
    const root = fs.realpathSync(tempDir())
    const projectDir = path.join(root, 'lib')
    fs.mkdirSync(path.join(projectDir, 'dist'), { recursive: true })

    expect(approveLegacyWorkspaceProjectBuilds({ 'lib@file:lib/dist': false }, {
      allProjects: [projectWithPublishDir(projectDir, 'dist')],
      lockfileDir: root,
    })).toStrictEqual({ 'lib@file:lib/dist': false })
  })

  test('that crosses a file is not approved and does not fail the deploy', () => {
    const root = fs.realpathSync(tempDir())
    const projectDir = path.join(root, 'lib')
    fs.mkdirSync(projectDir)
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    expect(approveLegacyWorkspaceProjectBuilds(undefined, {
      allProjects: [projectWithPublishDir(projectDir, 'package.json/dist')],
      lockfileDir: root,
    })).toStrictEqual({ 'lib@file:lib': true })
  })
})
