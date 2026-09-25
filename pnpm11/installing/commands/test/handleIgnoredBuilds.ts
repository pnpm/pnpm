import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { overrideTty } from '@pnpm/testing.command-defaults'
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

test('handleIgnoredBuilds adds placeholders to allowBuilds on an interactive terminal', async () => {
  prepareEmpty()
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { esbuild: false } })

  const restoreTty = overrideTty(true)
  try {
    await handleIgnoredBuilds({
      allowBuilds: { esbuild: false },
      ci: false,
      rootProjectManifestDir: process.cwd(),
    }, new Set(['esbuild@0.25.0' as DepPath, 'sharp@0.34.0' as DepPath]))
  } finally {
    restoreTty()
  }

  expect(readYamlFileSync('pnpm-workspace.yaml')).toStrictEqual({
    allowBuilds: {
      esbuild: false,
      sharp: 'set this to true or false',
    },
  })
})

test.each([
  { name: 'without a terminal', isTTY: false, ci: false },
  { name: 'in CI', isTTY: true, ci: true },
])('handleIgnoredBuilds does not update pnpm-workspace.yaml $name', async ({ isTTY, ci }) => {
  prepareEmpty()
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { esbuild: false } })
  const workspaceManifestBefore = fs.readFileSync('pnpm-workspace.yaml', 'utf8')

  const restoreTty = overrideTty(isTTY)
  try {
    await expect(handleIgnoredBuilds({
      allowBuilds: { esbuild: false },
      ci,
      rootProjectManifestDir: process.cwd(),
      strictDepBuilds: true,
    }, new Set(['sharp@0.34.0' as DepPath]))).rejects.toThrow('Ignored build scripts: sharp@0.34.0')
  } finally {
    restoreTty()
  }

  expect(fs.readFileSync('pnpm-workspace.yaml', 'utf8')).toBe(workspaceManifestBefore)
})
