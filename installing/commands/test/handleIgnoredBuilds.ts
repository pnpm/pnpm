import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import type { DepPath } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { handleIgnoredBuilds } from '../lib/handleIgnoredBuilds.js'

test('handleIgnoredBuilds does not update pnpm-workspace.yaml when workspace is ignored', async () => {
  prepareEmpty()

  const workspaceManifestFile = path.resolve('pnpm-workspace.yaml')
  const workspaceManifest = {
    allowBuilds: {
      esbuild: false,
    },
  }
  writeYamlFileSync(workspaceManifestFile, workspaceManifest)
  const workspaceManifestBefore = fs.readFileSync(workspaceManifestFile, 'utf8')

  await handleIgnoredBuilds({
    ignoreWorkspace: true,
    rootProjectManifestDir: process.cwd(),
  }, new Set(['esbuild@0.25.0' as DepPath]))

  expect(fs.readFileSync(workspaceManifestFile, 'utf8')).toBe(workspaceManifestBefore)
  expect(readYamlFileSync(workspaceManifestFile)).toStrictEqual(workspaceManifest)
})
