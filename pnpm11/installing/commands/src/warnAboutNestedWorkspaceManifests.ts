import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { logger } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import normalizePath from 'normalize-path'

/**
 * pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace
 * root. An installed project that has its own `pnpm-workspace.yaml` (a
 * nested workspace) gets a warning, because none of its settings, such as
 * `patchedDependencies` or `overrides`, apply when the outer workspace
 * installs it.
 */
export async function warnAboutNestedWorkspaceManifests (workspaceDir: string, projectDirs: string[]): Promise<void> {
  const nestedProjectDirs = projectDirs.filter((rootDir) => path.resolve(rootDir) !== path.resolve(workspaceDir))
  const withManifest = await Promise.all(nestedProjectDirs.map((rootDir) => isFile(path.join(rootDir, WORKSPACE_MANIFEST_FILENAME))))
  const relativeDirs = nestedProjectDirs
    .filter((_, index) => withManifest[index])
    .map((rootDir) => normalizePath(path.relative(workspaceDir, rootDir)))
    .sort(lexCompare)
  for (const relativeDir of relativeDirs) {
    logger.warn({
      message: `The settings in ${relativeDir}/${WORKSPACE_MANIFEST_FILENAME} do not apply, because ${relativeDir} is a project of this workspace. ` +
        `pnpm reads settings only from the ${WORKSPACE_MANIFEST_FILENAME} at the workspace root. ` +
        `Move the settings there, or add "!${relativeDir}" to the root's "packages" to keep that project a separate workspace.`,
      prefix: workspaceDir,
    })
  }
}

async function isFile (filePath: string): Promise<boolean> {
  try {
    return (await fs.promises.stat(filePath)).isFile()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) {
      return false
    }
    logger.debug({ error: err, message: `Could not stat nested workspace manifest at "${filePath}"` })
    return false
  }
}
