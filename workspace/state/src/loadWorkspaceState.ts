import fs from 'fs'
import util from 'util'
import { logger } from '@pnpm/logger'
import { getFilePath } from './filePath'
import { type WorkspaceState } from './types'

export function loadWorkspaceState (workspaceDir: string): WorkspaceState | undefined {
  logger.debug({ msg: 'loading workspace state' })
  const cacheFile = getFilePath(workspaceDir)
  let cacheFileContent: string
  try {
    cacheFileContent = fs.readFileSync(cacheFile, 'utf-8')
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
  return JSON.parse(cacheFileContent) as WorkspaceState
}
