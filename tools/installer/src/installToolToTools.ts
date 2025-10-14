import fs from 'fs'
import path from 'path'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { getToolDirPath } from '@pnpm/tools.path'
import { sync as rimraf } from '@zkochan/rimraf'
import { fastPathTemp as pathTemp } from 'path-temp'
import renameOverwrite from 'rename-overwrite'

export interface InstallToolToToolsOptions {
  pnpmHomeDir: string
  tool: {
    name: string
    version: string
  }
  /**
   * Additional arguments to pass to `pnpm add`.
   * Example: ['--allow-build=@pnpm/exe']
   */
  additionalPnpmAddArgs?: string[]
}

export interface InstallToolToToolsResult {
  alreadyExisted: boolean
  baseDir: string
  binDir: string
}

/**
 * Installs a tool to $PNPM_HOME/.tools/{name}/{version}/
 * This is a generic function used by both pnpm self-update and npm version management.
 */
export async function installToolToTools (
  opts: InstallToolToToolsOptions
): Promise<InstallToolToToolsResult> {
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: opts.tool,
  })

  const binDir = path.join(dir, 'bin')
  const alreadyExisted = fs.existsSync(binDir)
  if (alreadyExisted) {
    return {
      alreadyExisted,
      baseDir: dir,
      binDir,
    }
  }

  const stage = pathTemp(dir)
  fs.mkdirSync(stage, { recursive: true })
  fs.writeFileSync(path.join(stage, 'package.json'), '{}')

  try {
    const baseArgs = [
      'add',
      `${opts.tool.name}@${opts.tool.version}`,
      '--loglevel=error',
      '--no-dangerously-allow-all-builds',
      // We want to avoid symlinks because of the rename step,
      // which breaks the junctions on Windows.
      '--config.node-linker=hoisted',
      '--config.bin=bin',
    ]

    const args = opts.additionalPnpmAddArgs
      ? [...baseArgs, ...opts.additionalPnpmAddArgs]
      : baseArgs

    // The reason we don't just run add.handler is that at this point we might have settings from local config files
    // that we don't want to use while installing the tool.
    runPnpmCli(args, { cwd: stage })
    renameOverwrite.sync(stage, dir)
  } catch (err: unknown) {
    try {
      rimraf(stage)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }

  return {
    alreadyExisted: false,
    baseDir: dir,
    binDir,
  }
}
