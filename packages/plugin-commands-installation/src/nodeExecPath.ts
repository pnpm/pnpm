import { promises as fs } from 'fs'
import which from '@zkochan/which'

export default async function () {
  try {
    // The system default Node.js executable is prefered
    // not the one used to run the pnpm CLI.
    const nodeExecPath = await which('node')
    return fs.realpath(nodeExecPath)
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err
    return process.env.NODE ?? process.execPath
  }
}
