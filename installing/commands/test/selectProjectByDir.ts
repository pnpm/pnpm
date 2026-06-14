import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'

import { selectProjectByDir } from '../lib/selectProjectByDir.js'

test('selectProjectByDir keys the graph by the matched project root directory', () => {
  const rootDir = path.resolve('project') as ProjectRootDir
  const project: Project = {
    manifest: {},
    rootDir,
    rootDirRealPath: rootDir as unknown as ProjectRootDirRealPath,
    writeProjectManifest: async () => undefined,
  }

  const selectedProjectsGraph = selectProjectByDir([project], `${rootDir}${path.sep}`)

  expect(Object.keys(selectedProjectsGraph ?? {})).toStrictEqual([rootDir])
  expect(selectedProjectsGraph?.[rootDir].package).toBe(project)
})

test('selectProjectByDir returns undefined when no project matches', () => {
  const rootDir = path.resolve('project') as ProjectRootDir
  const project: Project = {
    manifest: {},
    rootDir,
    rootDirRealPath: rootDir as unknown as ProjectRootDirRealPath,
    writeProjectManifest: async () => undefined,
  }

  expect(selectProjectByDir([project], path.resolve('other-project'))).toBeUndefined()
})
