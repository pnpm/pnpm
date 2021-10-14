import { promises as fs } from 'fs'
import which from '@zkochan/which'

export default async function () {
  if (process['pkg'] != null) {
    const nodeExecPath = await which('node')
    return fs.realpath(nodeExecPath)
  }
  return process.env.NODE ?? process.execPath
}
