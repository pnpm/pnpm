import path from 'node:path'
import util from 'node:util'

import { readProjectManifest, type WriteProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'

export async function createProjectManifestWriter (projectDir: string): Promise<WriteProjectManifest> {
  try {
    const { writeProjectManifest } = await readProjectManifest(projectDir)
    return writeProjectManifest
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') {
      return writeProjectManifest.bind(null, path.join(projectDir, 'package.json')) as WriteProjectManifest
    }
    throw err
  }
}
