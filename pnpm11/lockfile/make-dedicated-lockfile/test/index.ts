import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { fixtures } from '@pnpm/test-fixtures'
import type { ProjectId } from '@pnpm/types'
import { safeExeca as execa } from 'execa'

const f = fixtures(import.meta.dirname)
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
const makeDedicatedLockfileBin = path.join(import.meta.dirname, '../bin/make-dedicated-lockfile.js')

test('make-dedicated-lockfile creates a dedicated lockfile', async () => {
  const tmp = f.prepare('fixture')
  await installWorkspace(tmp)
  const projectDir = path.join(tmp, 'packages/is-negative')
  await execa('node', [makeDedicatedLockfileBin], { cwd: projectDir })

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  // The next assertion started failing from pnpm v10.6.3
  // expect(Object.keys(lockfile?.importers ?? {})).toStrictEqual(['.', 'example'])
  expect(lockfile?.importers?.['.' as ProjectId]?.dependencies?.['is-positive']).toBe('link:../is-positive')
  expect(Object.keys(lockfile?.packages ?? {}).sort()).toStrictEqual([
    'lodash@1.0.0',
    'ramda@0.26.0',
    'request@2.0.0',
  ])
})

test('a workspace dependency stays linked instead of being fetched from the registry', async () => {
  const tmp = f.prepare('workspace-protocol')
  await installWorkspace(tmp)
  const projectDir = path.join(tmp, 'packages/app')
  const originalModulesMarker = path.join(projectDir, 'node_modules/original-tree')
  fs.writeFileSync(originalModulesMarker, '')
  const result = await execa('node', [makeDedicatedLockfileBin], { cwd: projectDir, all: true })
  expect(result.exitCode).toBe(0)
  expect(fs.existsSync(originalModulesMarker)).toBe(true)
  expect(fs.existsSync(path.join(projectDir, '.tmp_node_modules'))).toBe(false)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  expect(lockfile?.importers?.['.' as ProjectId]?.dependencies).toStrictEqual({
    '@dedicated-lockfile-test/lib': 'link:../shared',
    'is-positive': '1.0.0',
  })
  expect(lockfile?.importers?.['.' as ProjectId]?.specifiers).toStrictEqual({
    '@dedicated-lockfile-test/lib': 'workspace:^',
    'is-positive': '1.0.0',
  })
  expect(Object.keys(lockfile?.packages ?? {})).toStrictEqual(['is-positive@1.0.0'])
  expect(JSON.parse(fs.readFileSync(path.join(projectDir, 'package.json'), 'utf8')).dependencies['@dedicated-lockfile-test/lib']).toBe('workspace:^')

  await expect(installDedicatedCopy(tmp)).resolves.toStrictEqual(['@dedicated-lockfile-test', 'is-positive'])
})

test('a workspace dependency linked through linkWorkspacePackages stays linked', async () => {
  const tmp = f.prepare('linked-by-range')
  await installWorkspace(tmp)
  const projectDir = path.join(tmp, 'packages/app')
  const result = await execa('node', [makeDedicatedLockfileBin], { cwd: projectDir, all: true })
  expect(result.exitCode).toBe(0)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  expect(lockfile?.importers?.['.' as ProjectId]?.dependencies).toStrictEqual({
    '@dedicated-lockfile-test/range-lib': 'link:../shared',
    'is-positive': '1.0.0',
  })
  expect(Object.keys(lockfile?.packages ?? {})).toStrictEqual(['is-positive@1.0.0'])

  await expect(installDedicatedCopy(tmp)).resolves.toStrictEqual(['@dedicated-lockfile-test', 'is-positive'])
})

test('a workspace peer dependency stays linked', async () => {
  const tmp = f.prepare('workspace-peer')
  await installWorkspace(tmp)
  const projectDir = path.join(tmp, 'packages/app')
  const result = await execa('node', [makeDedicatedLockfileBin], { cwd: projectDir, all: true })
  expect(result.exitCode).toBe(0)

  const lockfile = await readWantedLockfile(projectDir, { ignoreIncompatible: false })
  expect(lockfile?.importers?.['.' as ProjectId]?.dependencies).toStrictEqual({
    '@dedicated-lockfile-test/peer-lib': 'link:../shared',
    'is-positive': '1.0.0',
  })
  expect(JSON.parse(fs.readFileSync(path.join(projectDir, 'package.json'), 'utf8')).peerDependencies['@dedicated-lockfile-test/peer-lib']).toBe('workspace:^')
})

test('a node_modules left staged by an earlier run is not overwritten', async () => {
  const tmp = f.prepare('workspace-protocol')
  await installWorkspace(tmp)
  const projectDir = path.join(tmp, 'packages/app')
  const stagedMarker = path.join(projectDir, '.tmp_node_modules/original-tree')
  fs.mkdirSync(path.dirname(stagedMarker))
  fs.writeFileSync(stagedMarker, '')
  const manifestBefore = fs.readFileSync(path.join(projectDir, 'package.json'), 'utf8')

  const result = await execa('node', [makeDedicatedLockfileBin], { cwd: projectDir, all: true, reject: false })

  expect(result.exitCode).not.toBe(0)
  expect(result.all).toContain('ERR_PNPM_STAGED_MODULES_DIR_EXISTS')
  expect(fs.existsSync(stagedMarker)).toBe(true)
  expect(fs.readFileSync(path.join(projectDir, 'package.json'), 'utf8')).toBe(manifestBefore)
})

async function installWorkspace (workspaceDir: string): Promise<void> {
  await execa('node', [
    pnpmBin,
    '--config.store-dir=store',
    '--config.cache-dir=cache',
    'install',
    '--no-frozen-lockfile',
    '--no-prefer-frozen-lockfile',
    '--force',
  ], { cwd: workspaceDir })
}

// Copies the project and its linked dependency out of the workspace, the way a
// Docker build would, and runs a frozen install from the dedicated lockfile.
async function installDedicatedCopy (workspaceDir: string): Promise<string[]> {
  const copyDir = `${workspaceDir}-copy`
  const skipModules = (src: string) => !src.includes('node_modules')
  for (const project of ['app', 'shared']) {
    fs.cpSync(path.join(workspaceDir, 'packages', project), path.join(copyDir, project), { recursive: true, filter: skipModules })
  }
  const appDir = path.join(copyDir, 'app')
  await execa('node', [
    pnpmBin,
    `--config.store-dir=${path.join(workspaceDir, 'store')}`,
    `--config.cache-dir=${path.join(workspaceDir, 'cache')}`,
    'install',
    '--frozen-lockfile',
  ], { cwd: appDir })
  return fs.readdirSync(path.join(appDir, 'node_modules')).filter((entry) => !entry.startsWith('.')).sort()
}
