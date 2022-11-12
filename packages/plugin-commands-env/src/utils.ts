import { promises as fs } from 'fs'
import path from 'path'

export async function getNodeExecPathAndTargetDir (pnpmHomeDir: string) {
  const nodePath = getNodeExecPathInBinDir(pnpmHomeDir)
  let nodeLink: string | undefined
  try {
    nodeLink = await fs.readlink(nodePath)
  } catch (err) {
    nodeLink = undefined
  }
  return { nodePath, nodeLink }
}

export function getNodeExecPathInBinDir (pnpmHomeDir: string) {
  return path.resolve(pnpmHomeDir, process.platform === 'win32' ? 'node.exe' : 'node')
}

export function getNodeExecPathInNodeDir (nodeDir: string) {
  return path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
}
