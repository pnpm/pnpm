import { execFile } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { expect, test } from '@jest/globals'
import {
  applyReleasePlan,
  assembleReleasePlan,
  readChangeIntents,
  readLedger,
  type WorkspaceProject,
} from '@pnpm/releasing.versioning'
import { temporaryDirectory } from 'tempy'

const execFileAsync = util.promisify(execFile)
const changesetBin = path.join(import.meta.dirname, '..', 'node_modules', '.bin', 'changeset')

const packageNames = ['a', 'b', 'c', 'd']

test('native direct bumps and fixed groups match Changesets manifests and changelogs', async () => {
  const nativeDir = temporaryDirectory()
  const changesetsDir = temporaryDirectory()
  writeFixture(nativeDir)
  writeFixture(changesetsDir)

  const projects = readProjects(nativeDir)
  const intents = await readChangeIntents(nativeDir)
  const versioning = {
    fixed: [['c', 'd']],
    changelog: { storage: 'repository' as const },
  }
  const plan = assembleReleasePlan({
    workspaceDir: nativeDir,
    projects,
    intents,
    ledger: await readLedger(nativeDir),
    versioning,
    enforceWorkspaceProtocol: true,
  })
  await applyReleasePlan(plan, {
    workspaceDir: nativeDir,
    projects,
    allIntents: intents,
    versioning,
  })

  await execFileAsync(changesetBin, ['version'], { cwd: changesetsDir })

  for (const name of packageNames) {
    const nativeManifest = readJson(path.join(nativeDir, 'packages', name, 'package.json'))
    const changesetsManifest = readJson(path.join(changesetsDir, 'packages', name, 'package.json'))
    expect(nativeManifest).toEqual(changesetsManifest)
    expect(readNormalized(path.join(nativeDir, 'packages', name, 'CHANGELOG.md')))
      .toBe(readNormalized(path.join(changesetsDir, 'packages', name, 'CHANGELOG.md')))
  }
})

function writeFixture (workspaceDir: string): void {
  fs.mkdirSync(path.join(workspaceDir, '.changeset'), { recursive: true })
  fs.writeFileSync(path.join(workspaceDir, 'package.json'), JSON.stringify({
    name: 'differential-root',
    private: true,
  }, null, 2))
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  fs.writeFileSync(path.join(workspaceDir, '.changeset', 'config.json'), JSON.stringify({
    changelog: '@changesets/cli/changelog',
    commit: false,
    fixed: [['c', 'd']],
    linked: [],
    access: 'restricted',
    baseBranch: 'main',
    updateInternalDependencies: 'patch',
    ignore: [],
  }, null, 2))
  fs.writeFileSync(path.join(workspaceDir, '.changeset', 'parity.md'), `---
"a": minor
"b": patch
"c": minor
---
Added the parity fixture.
`)
  writePackage(workspaceDir, 'a', '1.0.0')
  writePackage(workspaceDir, 'b', '1.0.0', { a: 'workspace:' })
  writePackage(workspaceDir, 'c', '2.0.0')
  writePackage(workspaceDir, 'd', '2.0.0')
}

function writePackage (
  workspaceDir: string,
  name: string,
  version: string,
  dependencies?: Record<string, string>
): void {
  const dir = path.join(workspaceDir, 'packages', name)
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name,
    version,
    ...(dependencies == null ? {} : { dependencies }),
  }, null, 2))
}

function readProjects (workspaceDir: string): WorkspaceProject[] {
  return packageNames.map((name) => ({
    rootDir: path.join(workspaceDir, 'packages', name),
    manifest: readJson(path.join(workspaceDir, 'packages', name, 'package.json')),
  }))
}

function readJson (filePath: string): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(filePath, 'utf8')) as Record<string, unknown>
}

function readNormalized (filePath: string): string {
  return fs.readFileSync(filePath, 'utf8').replaceAll('\r\n', '\n').trim()
}
