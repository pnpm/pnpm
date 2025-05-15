import path from 'path'
import util from 'util'
import { readProjectManifest, type WriteProjectManifest } from '@pnpm/read-project-manifest'
import { writeProjectManifest } from '@pnpm/write-project-manifest'

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
