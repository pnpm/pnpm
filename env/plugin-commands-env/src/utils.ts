import { promises as fs } from 'fs'
import path from 'path'

export const CURRENT_NODE_DIRNAME = 'nodejs_current'

export async function getNodeExecPathAndTargetDir (pnpmHomeDir: string) {
  const nodePath = getNodeExecPathInBinDir(pnpmHomeDir)
  const nodeCurrentDirLink = path.join(pnpmHomeDir, CURRENT_NODE_DIRNAME)
  let nodeCurrentDir: string | undefined
  try {
    nodeCurrentDir = await fs.readlink(nodeCurrentDirLink)
  } catch (err) {
    nodeCurrentDir = undefined
  }
  return { nodePath, nodeLink: nodeCurrentDir ? getNodeExecPathInNodeDir(nodeCurrentDir) : undefined }
}

export function getNodeExecPathInBinDir (pnpmHomeDir: string) {
  return path.resolve(pnpmHomeDir, process.platform === 'win32' ? 'node.exe' : 'node')
}

export function getNodeExecPathInNodeDir (nodeDir: string) {
  return path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
}
