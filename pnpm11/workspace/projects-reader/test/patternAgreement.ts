/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { isWorkspaceProjectDir } from '@pnpm/workspace.package-patterns'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { temporaryDirectory } from 'tempy'

/**
 * Every directory the walk could return for the workspace below, so the
 * agreement check covers the ones a pattern leaves out as well.
 */
const PROJECT_DIRS = [
  '.',
  'packages/a',
  'packages/a/nested',
  'packages/b',
  'examples/example-1',
  'docs',
  'libs/yaml-only',
  'node_modules/dep',
  '.hidden/tool',
]

test.each([
  [['.']],
  [['packages/*']],
  [['packages/**']],
  [['**']],
  [['**', '!examples/**']],
  [['**', '!/examples/**']],
  [['./packages/a']],
  [['packages/missing/../a']],
  [['packages/**', 'libs/*']],
  [['!packages/**']],
])('agrees with the workspace walk on %s', async (patterns) => {
  const workspaceDir = prepareWorkspace()
  const selected = new Set((await findWorkspaceProjectsNoCheck(workspaceDir, { patterns }))
    .map(({ rootDir }) => rootDir as string))

  for (const projectDir of PROJECT_DIRS) {
    const dir = path.resolve(workspaceDir, projectDir)
    expect({ projectDir, isProject: isWorkspaceProjectDir({ workspaceDir, dir, patterns }) })
      .toStrictEqual({ projectDir, isProject: selected.has(dir) })
  }
})

function prepareWorkspace (): string {
  const workspaceDir = fs.realpathSync.native(temporaryDirectory())
  for (const projectDir of PROJECT_DIRS) {
    const dir = path.resolve(workspaceDir, projectDir)
    fs.mkdirSync(dir, { recursive: true })
    const manifestName = projectDir === 'libs/yaml-only' ? 'package.yaml' : 'package.json'
    fs.writeFileSync(path.join(dir, manifestName), manifestName === 'package.yaml'
      ? 'name: yaml-only\nversion: 0.0.0\n'
      : '{"name":"test","version":"0.0.0"}')
  }
  return workspaceDir
}
