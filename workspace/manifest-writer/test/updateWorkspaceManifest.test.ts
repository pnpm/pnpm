import path from 'path'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { updateWorkspaceManifest } from '@pnpm/workspace.manifest-writer'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as writeYamlFile } from 'write-yaml-file'

test('updateWorkspaceManifest', async () => {
  const dir = tempDir(false)
  const filePath = path.join(dir, WORKSPACE_MANIFEST_FILENAME)
  writeYamlFile(filePath, { packages: ['*'] })
  await updateWorkspaceManifest(dir, {
    onlyBuiltDependencies: [],
  })
  expect(readYamlFile(filePath)).toStrictEqual({
    packages: ['*'],
    onlyBuiltDependencies: [],
  })
})
