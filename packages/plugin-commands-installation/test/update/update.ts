import path from 'path'
import PnpmError from '@pnpm/error'
import { install, update } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { ProjectManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'
import { DEFAULT_OPTS } from '../utils'

test('update with "*" pattern', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })

  const project = prepare({
    dependencies: {
      'peer-a': '1.0.0',
      'peer-c': '1.0.0',
      'pnpm-foo': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  })

  await update.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    workspaceDir: process.cwd(),
  }, ['peer-*'])

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/peer-a/1.0.1']).toBeTruthy()
  expect(lockfile.packages['/peer-c/2.0.0']).toBeTruthy()
  expect(lockfile.packages['/pnpm-foo/1.0.0']).toBeTruthy()
})

test('update: fail when both "latest" and "workspace" are true', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await update.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project-1'),
      latest: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
      workspaceDir: process.cwd(),
    }, ['project-2'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_BAD_OPTIONS')
  expect(err.message).toBe('Cannot use --latest with --workspace simultaneously')
})

describe('update by package name', () => {
  beforeAll(async () => {
    prepare({
      dependencies: {
        'peer-a': '1.0.0',
        'peer-c': '1.0.0',
      },
    })
    await install.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      workspaceDir: process.cwd(),
    })
  })
  it("should fail when the package isn't in the direct dependencies and depth is 0", async () => {
    let err!: PnpmError
    try {
      await update.handler({
        ...DEFAULT_OPTS,
        depth: 0,
        dir: process.cwd(),
        sharedWorkspaceLockfile: true,
        workspaceDir: process.cwd(),
      }, ['peer-b'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES')
    expect(err.message).toBe('None of the specified packages were found in the dependencies of any of the projects.')
  })
  it("shouldn't fail when the package isn't in the direct dependencies", async () => {
    await update.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      sharedWorkspaceLockfile: true,
      workspaceDir: process.cwd(),
    }, ['peer-b'])
  })
})

test('update --no-save should not update package.json and pnpm-lock.yaml', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })

  const project = prepare({
    dependencies: {
      'peer-a': '^1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  })

  {
    const manifest = await loadJsonFile<ProjectManifest>('package.json')
    expect(manifest.dependencies?.['peer-a']).toBe('^1.0.0')

    const lockfile = await project.readLockfile()
    expect(lockfile.specifiers['peer-a']).toBe('^1.0.0')
    expect(lockfile.packages['/peer-a/1.0.0']).toBeTruthy()
  }

  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    save: false,
    workspaceDir: process.cwd(),
  }, [])

  {
    const manifest = await loadJsonFile<ProjectManifest>('package.json')
    expect(manifest.dependencies?.['peer-a']).toBe('^1.0.0')

    const lockfile = await project.readLockfile()
    expect(lockfile.specifiers['peer-a']).toBe('^1.0.0')
    expect(lockfile.packages['/peer-a/1.0.1']).toBeTruthy()
  }
})

// fix: https://github.com/pnpm/pnpm/issues/4196
test('update should work normal when set empty string version', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })

  const project = prepare({
    dependencies: {
      'peer-a': '1.0.0',
    },
    devDependencies: {
      'pnpm-foo': '',
      'peer-c': '',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  })

  await update.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    workspaceDir: process.cwd(),
  }, ['*'])

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/peer-a/1.0.1']).toBeTruthy()
  expect(lockfile.packages['/peer-c/2.0.0']).toBeTruthy()
  expect(lockfile.packages['/pnpm-foo/2.0.0']).toBeTruthy()
  expect(lockfile.dependencies['peer-a']).toEqual('1.0.1')
  expect(lockfile.devDependencies['pnpm-foo']).toEqual('2.0.0')
  expect(lockfile.devDependencies['peer-c']).toEqual('2.0.0')
})
