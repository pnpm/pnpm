import fs from 'node:fs'
import path from 'node:path'
import semver from 'semver'
import { spawn } from 'node:child_process'
import { DevEngines } from '@pnpm/types'
import { getNodeExecPathInBinDir } from '../../env/plugin-commands-env/src/utils.js'
import { getNodeBinDir } from '../../env/plugin-commands-env/src/node.js'
import { getConfig } from '@pnpm/config'
import { packageManager } from '@pnpm/cli-meta'

/**
 * Switches the Node runtime to the version specified in `devEngines.runtime` of package.json.
 * Spawns a child process if a switch is needed.
 *
 * @returns {Promise<boolean>} - true if a switch occurred and the current process will exit
 */
export async function switchNodeBasedOnDevEngine (): Promise<boolean> {
  const pkgPath = path.resolve(process.cwd(), 'package.json')
  if (!fs.existsSync(pkgPath)) return false

  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8')) as { devEngines?: DevEngines }
  const runtime = pkg.devEngines?.runtime
  if (!runtime) return false

  const wantedNodeVersion = (Array.isArray(runtime) ? runtime : [runtime])
    .find(rt => rt.name === 'node')?.version
  if (!wantedNodeVersion) return false
  if (process.env.PNPM_NODE_SWITCHED) return false
  if (semver.satisfies(process.version.slice(1), wantedNodeVersion)) return false


  const { config } = await getConfig({
    cliOptions: {},
    packageManager,
  })

  const nodeBinDir = await getNodeBinDir({
    useNodeVersion: wantedNodeVersion,
    global: true,
    pnpmHomeDir: config.pnpmHomeDir,
    bin: path.join(config.pnpmHomeDir, 'bin'),
    rawConfig: {},
  })

  const nodeExecPath = getNodeExecPathInBinDir(nodeBinDir)

  const child = spawn(nodeExecPath, process.argv.slice(1), {
    stdio: 'inherit',
    env: { ...process.env, PNPM_NODE_SWITCHED: '1' },
  })

  child.on('exit', code => process.exit(code ?? 0))
  child.on('error', err => {
    console.error('Failed to spawn Node process:', err)
    process.exit(1)
  })

  return true
}
