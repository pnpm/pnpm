import { promises as fs } from 'fs'
import which from '@zkochan/which'

export async function getNodeExecPath () {
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
