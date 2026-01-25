/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import os from 'os'
import path from 'path'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'

describe('invalid workspace manifest filenames', () => {
  const invalidFilenames = [
    'pnpm-workspaces.yaml',
    'pnpm-workspaces.yml',
    'pnpm-workspace.yml',
    '.pnpm-workspace.yaml',
    '.pnpm-workspace.yml',
  ]

  for (const filename of invalidFilenames) {
    test(`throws error for ${filename}`, async () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-'))
      fs.writeFileSync(path.join(tempDir, filename), '')

      try {
        await expect(findWorkspaceDir(tempDir)).rejects.toMatchObject({
          code: 'ERR_PNPM_BAD_WORKSPACE_MANIFEST_NAME',
          message: expect.stringContaining('The workspace manifest file should be named "pnpm-workspace.yaml"'),
        })
      } finally {
        fs.rmSync(tempDir, { recursive: true })
      }
    })
  }
})
