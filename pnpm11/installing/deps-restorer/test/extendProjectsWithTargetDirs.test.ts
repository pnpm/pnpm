import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectId } from '@pnpm/types'

import { extendProjectsWithTargetDirs } from '../src/extendProjectsWithTargetDirs.js'

test('a directory dependency named like an Object.prototype key is not taken for a project', () => {
  const lockfileDir = path.resolve('/workspace')
  const projects = [{ id: '.' as ProjectId, rootDir: lockfileDir }]
  const injectionTargetsByDepPath = new Map([
    ['foo@file:constructor', [path.join(lockfileDir, 'node_modules/foo')]],
  ])

  expect(extendProjectsWithTargetDirs(projects, injectionTargetsByDepPath, lockfileDir)).toStrictEqual([
    { id: '.', rootDir: lockfileDir, targetDirs: [], publishTargetDirs: [] },
  ])
})
