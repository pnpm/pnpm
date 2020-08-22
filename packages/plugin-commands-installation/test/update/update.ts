import PnpmError from '@pnpm/error'
import { install, update } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { ProjectManifest } from '@pnpm/types'
import { DEFAULT_OPTS } from '../utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import test = require('tape')

test('update with "*" pattern', async (t) => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })

  const project = prepare(t, {
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

  t.ok(lockfile.packages['/peer-a/1.0.1'])
  t.ok(lockfile.packages['/peer-c/2.0.0'])
  t.ok(lockfile.packages['/pnpm-foo/1.0.0'])
  t.end()
})

test('update: fail when both "latest" and "workspace" are true', async (t) => {
  preparePackages(t, [
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
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_BAD_OPTIONS')
  t.equal(err.message, 'Cannot use --latest with --workspace simultaneously')
  t.end()
})

test('update: fail when package not in dependencies', async (t) => {
  prepare(t, {
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

  let err!: PnpmError
  try {
    await update.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      sharedWorkspaceLockfile: true,
      workspaceDir: process.cwd(),
    }, ['peer-b'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES')
  t.equal(err.message, 'None of the specified packages were found in the dependencies.')
  t.end()
})

test('update --no-save should not update package.json and pnpm-lock.yaml', async (t) => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })

  const project = prepare(t, {
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
    t.equal(manifest.dependencies['peer-a'], '^1.0.0')

    const lockfile = await project.readLockfile()
    t.equal(lockfile.specifiers['peer-a'], '^1.0.0')
    t.ok(lockfile.packages['/peer-a/1.0.0'])
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
    t.equal(manifest.dependencies['peer-a'], '^1.0.0')

    const lockfile = await project.readLockfile()
    t.equal(lockfile.specifiers['peer-a'], '^1.0.0')
    t.ok(lockfile.packages['/peer-a/1.0.1'])
  }

  t.end()
})
