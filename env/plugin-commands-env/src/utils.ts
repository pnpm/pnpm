import path from 'path'

export function getNodeExecPathInBinDir (pnpmHomeDir: string): string {
  return path.resolve(pnpmHomeDir, process.platform === 'win32' ? 'node.exe' : 'node')
}

export function getNodeExecPathInNodeDir (nodeDir: string): string {
  return path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
}
