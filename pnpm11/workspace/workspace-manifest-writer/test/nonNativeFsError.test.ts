import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { GLOBAL_CONFIG_YAML_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { readYamlFileSync } from 'read-yaml-file'

// StackBlitz WebContainers throw fs errors that carry the usual errno fields
// but are not native errors (util.types.isNativeError() returns false).
function toNonNativeError (err: unknown): unknown {
  if (err == null || typeof err !== 'object') return err
  return Object.assign(Object.create(Error.prototype) as Error, err, { message: (err as Error).message })
}

jest.unstable_mockModule('node:fs', () => ({
  default: {
    ...fs,
    promises: {
      ...fs.promises,
      readFile: async (...args: Parameters<typeof fs.promises.readFile>) => {
        try {
          return await fs.promises.readFile(...args)
        } catch (err) {
          throw toNonNativeError(err)
        }
      },
    },
  },
}))

const { updateWorkspaceManifest } = await import('@pnpm/workspace.workspace-manifest-writer')

test('updateWorkspaceManifest creates a missing file when the fs error is not a native error', async () => {
  const dir = tempDir(false)
  await updateWorkspaceManifest(dir, {
    fileName: GLOBAL_CONFIG_YAML_FILENAME,
    updatedFields: { overrides: { foo: '1.0.0' } },
  })
  expect(readYamlFileSync(path.join(dir, GLOBAL_CONFIG_YAML_FILENAME))).toStrictEqual({ overrides: { foo: '1.0.0' } })
})
