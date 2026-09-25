import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

test('does not discover nested projects when the workspace manifest has no packages field', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-workspace-filter-'))
  try {
    await fs.mkdir(path.join(workspaceDir, 'nested'), { recursive: true })
    await Promise.all([
      fs.writeFile(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' })),
      fs.writeFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'minimumReleaseAge: 0\n'),
      fs.writeFile(path.join(workspaceDir, 'nested/package.json'), JSON.stringify({ name: 'nested' })),
    ])

    const result = await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

    expect(result.allProjects.map(({ manifest }) => manifest.name)).toStrictEqual(['root'])
  } finally {
    await fs.rm(workspaceDir, { recursive: true, force: true })
  }
})

test('discovers nested projects when there is no workspace manifest', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-workspace-filter-'))
  try {
    await fs.mkdir(path.join(workspaceDir, 'nested'), { recursive: true })
    await Promise.all([
      fs.writeFile(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' })),
      fs.writeFile(path.join(workspaceDir, 'nested/package.json'), JSON.stringify({ name: 'nested' })),
    ])

    const result = await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

    expect(result.allProjects.map(({ manifest }) => manifest.name)).toStrictEqual(['root', 'nested'])
  } finally {
    await fs.rm(workspaceDir, { recursive: true, force: true })
  }
})
