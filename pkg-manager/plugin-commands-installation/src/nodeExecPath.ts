import { promises as fs } from 'fs'
import which from 'which'

export async function getNodeExecPath (): Promise<string> {
  try {
    // The system default Node.js executable is preferred
    // not the one used to run the pnpm CLI.
    const nodeExecPath = await which('node')
    return fs.realpath(nodeExecPath)
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
    return process.env.NODE ?? process.execPath
  }
}
