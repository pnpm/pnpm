import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { filterProjectsBySelectorObjects } from '@pnpm/workspace.projects-filter'
import type { BaseProject } from '@pnpm/workspace.projects-graph'
import { safeExeca as execa } from 'execa'
import { temporaryDirectory } from 'tempy'

test('catalog changes select child directories starting with two dots without selecting siblings', async () => {
  const workspaceDir = temporaryDirectory()
  const parentDir = path.join(workspaceDir, 'packages')
  const childDir = path.join(parentDir, '..foo') as ProjectRootDir
  const regularDir = path.join(parentDir, 'regular') as ProjectRootDir
  const siblingDir = path.join(workspaceDir, 'packages-other', 'sibling') as ProjectRootDir
  const projects: BaseProject[] = [childDir, regularDir, siblingDir].map((rootDir, index) => ({
    rootDir,
    manifest: {
      name: `project-${index}`,
      version: '1.0.0',
      dependencies: { foo: 'catalog:' },
    },
  }))

  try {
    await Promise.all(projects.map(async (project) => {
      await fs.mkdir(project.rootDir, { recursive: true })
      await fs.writeFile(path.join(project.rootDir, 'package.json'), JSON.stringify(project.manifest))
    }))
    const workspaceManifestPath = path.join(workspaceDir, 'pnpm-workspace.yaml')
    const workspaceManifest = (version: string) => `packages:
  - 'packages/**'
  - 'packages-other/*'
catalog:
  foo: ${version}
`
    await fs.writeFile(workspaceManifestPath, workspaceManifest('^1.0.0'))
    await execa('git', ['init', '--initial-branch=main'], { cwd: workspaceDir })
    await execa('git', ['config', 'user.email', 'x@y.z'], { cwd: workspaceDir })
    await execa('git', ['config', 'user.name', 'xyz'], { cwd: workspaceDir })
    await execa('git', ['add', '.'], { cwd: workspaceDir })
    await execa('git', ['commit', '-m', 'initial', '--no-gpg-sign'], { cwd: workspaceDir })
    await fs.writeFile(workspaceManifestPath, workspaceManifest('^1.1.0'))

    const { selectedProjectsGraph } = await filterProjectsBySelectorObjects(projects, [{
      diff: 'HEAD',
      parentDir,
    }], { workspaceDir, linkWorkspacePackages: true })

    expect(Object.keys(selectedProjectsGraph).sort()).toStrictEqual([childDir, regularDir].sort())
  } finally {
    await fs.rm(workspaceDir, { recursive: true, force: true })
  }
})
