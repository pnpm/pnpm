import { promises as fs } from 'fs'
import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli-meta'
import which from 'which'

export async function getNodeExecPath (): Promise<string | undefined> {
  try {
    // The system default Node.js executable is preferred
    // not the one used to run the pnpm CLI.
    const nodeExecPath = await which('node')
    return fs.realpath(nodeExecPath)
  } catch (err: any) { // eslint-disable-line
    if (err['code'] !== 'ENOENT') throw err
    // When pnpm runs as @pnpm/exe (a Single Executable Application that
    // bundles Node.js into the pnpm binary), process.execPath points at the
    // pnpm binary itself rather than a standalone Node binary. Using it as
    // the nodeExecPath of a bin shim makes the shim invoke pnpm instead of
    // Node, which breaks globally-installed CLIs. See #11291 and #4645.
    if (detectIfCurrentPkgIsExecutable()) return undefined
    return process.env.NODE ?? process.execPath
  }
}
