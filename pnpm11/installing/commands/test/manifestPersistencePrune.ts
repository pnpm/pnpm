import fs from 'node:fs'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const actualCliUtils = await import('@pnpm/cli.utils')
const actualPruning = await import('../src/resolvedPackageVersionsForPrune.js')
const writeProjectManifest = jest.fn<(manifest: ProjectManifest) => Promise<void>>()
const collectVersions = jest.fn(actualPruning.resolvedPackageVersionsForPrune)

jest.unstable_mockModule('@pnpm/cli.utils', () => ({
  ...actualCliUtils,
  tryReadProjectManifest: async (...args: Parameters<typeof actualCliUtils.tryReadProjectManifest>) => ({
    ...await actualCliUtils.tryReadProjectManifest(...args),
    writeProjectManifest,
  }),
  readProjectManifest: async (...args: Parameters<typeof actualCliUtils.readProjectManifest>) => ({
    ...await actualCliUtils.readProjectManifest(...args),
    writeProjectManifest,
  }),
}))
jest.unstable_mockModule('../src/resolvedPackageVersionsForPrune.js', () => ({
  ...actualPruning,
  resolvedPackageVersionsForPrune: collectVersions,
}))

const { add, remove, update } = await import('../src/index.js')

beforeEach(() => {
  writeProjectManifest.mockReset().mockResolvedValue(undefined)
  collectVersions.mockReset().mockRejectedValue(new Error('env lockfile read failed'))
})

test.each(['add', 'update', 'remove', 'recursive'] as const)('%s reads exclusion versions before starting manifest writes', async (command) => {
  if (command === 'recursive') {
    preparePackages([{ name: 'project-1', version: '1.0.0' }, { name: 'project-2', version: '1.0.0' }])
    fs.writeFileSync('pnpm-workspace.yaml', "packages: ['*']\n")
    const projects = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
    for (const project of projects.allProjects) project.writeProjectManifest = writeProjectManifest

    await expect(add.handler({
      ...DEFAULT_OPTS,
      ...projects,
      dir: process.cwd(),
      workspaceDir: process.cwd(),
      lockfileDir: process.cwd(),
      sharedWorkspaceLockfile: true,
      recursive: true,
      minimumReleaseAgeExcludePrune: true,
    }, ['is-positive@1.0.0'])).rejects.toThrow('env lockfile read failed')
  } else {
    prepare({ name: 'project', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } })
    fs.writeFileSync('pnpm-workspace.yaml', "packages: ['.']\n")
    const opts = {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      minimumReleaseAgeExcludePrune: true,
    }
    await expect(command === 'add'
      ? add.handler(opts, ['is-positive@1.0.0'])
      : command === 'remove'
        ? remove.handler(opts, ['is-positive'])
        : update.handler(opts, []))
      .rejects.toThrow('env lockfile read failed')
  }

  expect(collectVersions).toHaveBeenCalled()
  expect(writeProjectManifest).not.toHaveBeenCalled()
})
