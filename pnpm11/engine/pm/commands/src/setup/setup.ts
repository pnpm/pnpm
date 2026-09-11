import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { detectIfCurrentPkgIsExecutable, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { logger } from '@pnpm/logger'
import {
  addDirToEnvPath,
  type ConfigReport,
  type PathExtenderReport,
} from '@pnpm/os.env.path-extender'
import PATH from 'path-name'
import { renderHelp } from 'render-help'

import {
  validateGHActionsEnvFileValues,
  writeGHActionsEnvFiles,
} from './ghActionsEnv.js'

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export const cliOptionsTypes = (): Record<string, unknown> => ({
  force: Boolean,
})

export const shorthands = {}

export const commandNames = ['setup']

export const overridableByScript = true

export function help (): string {
  return renderHelp({
    description: 'Sets up pnpm',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Override the PNPM_HOME env variable in case it already exists',
            name: '--force',
            shortAlias: '-f',
          },
        ],
      },
    ],
    url: docsUrl('setup'),
    usages: ['pnpm setup'],
  })
}

function getExecPath (): string {
  if (detectIfCurrentPkgIsExecutable()) {
    // If the pnpm CLI is a single executable application (SEA), we use the path
    // to the exe file instead of the js path.
    return process.execPath
  }
  return process.argv[1] ?? process.cwd()
}

/**
 * Install the CLI as a global package using `pnpm add -g file:<dir>`.
 * This places pnpm in the standard global directory alongside other
 * globally installed packages.
 */
function installCliGlobally (execPath: string, pnpmHomeDir: string): void {
  const execDir = path.dirname(execPath)
  const execName = path.basename(execPath)
  const pkgJsonPath = path.join(execDir, 'package.json')

  // Write a package.json if one doesn't already exist.
  // (Updated tarballs on GitHub Pages will ship with package.json already.)
  let createdPkgJson = false
  if (!fs.existsSync(pkgJsonPath)) {
    fs.writeFileSync(pkgJsonPath, JSON.stringify(standaloneManifest(execName)))
    createdPkgJson = true
  }

  logger.info({
    message: `Installing pnpm CLI globally from ${execDir}`,
    prefix: process.cwd(),
  })

  try {
    const binDir = path.join(pnpmHomeDir, 'bin')
    // @pnpm/exe ships a `preinstall`/`prepare` pair (setup.js/prepare.js) that
    // hardlinks the platform-specific binary out of its optional platform
    // packages. None of that applies here: this `file:` dependency is the
    // standalone executable itself (its binary is already present), the
    // platform packages aren't installed alongside it, and the SEA host may
    // have no `node` to run the scripts at all. Skipping them avoids a build
    // approval prompt for pnpm's own install. See
    // https://github.com/pnpm/pnpm/issues/12377.
    const { status, error } = spawnSync(execPath, ['add', '-g', '--ignore-scripts', `file:${execDir}`], {
      stdio: 'inherit',
      env: {
        ...process.env,
        PNPM_HOME: pnpmHomeDir,
        [PATH]: `${binDir}${path.delimiter}${process.env[PATH] ?? ''}`,
      },
    })

    if (error) throw error
    if (status !== 0) {
      throw new Error(`Failed to install pnpm globally (exit code ${status})`)
    }
  } finally {
    if (createdPkgJson) {
      fs.unlinkSync(pkgJsonPath)
    }
  }
}

/**
 * The manifest `pnpm setup` writes next to a standalone executable that ships
 * without one, so the global install has a package to install.
 *
 * `type: module` matters even though nothing here is imported as a package:
 * without it Node.js reparses the ESM files shipped alongside the executable
 * (`dist/worker.js`) as CommonJS first and warns on every spawn.
 */
export function standaloneManifest (execName: string): {
  name: string
  version: string
  type: string
  bin: Record<string, string>
  files: string[]
} {
  return {
    name: '@pnpm/exe',
    version: packageManager.version,
    type: 'module',
    bin: { pnpm: execName, pn: execName },
    files: [execName, 'dist/'],
  }
}

function createAliasScripts (targetDir: string): void {
  // Why script files instead of aliases?
  // 1. Aliases wouldn't work on all platform, such as Windows Command Prompt or POSIX `sh`.
  // 2. Aliases wouldn't work on all environments, such as non-interactive shells and CI environments.
  // 3. Aliases must be set for different shells while script files are limited to only 2 types: POSIX and Windows.
  // 4. Aliases cannot be located with the `which` or `where` command.
  // 5. Editing rc files is more error-prone than just write new files to the filesystem.

  fs.mkdirSync(targetDir, { recursive: true })

  createShellScript(targetDir, 'pn', '')
  createShellScript(targetDir, 'pnpx', ' dlx')
  createShellScript(targetDir, 'pnx', ' dlx')
}

/**
 * Write one alias, `subcommand` being the shell text it appends to the pnpm call
 * (`' dlx'` for `pnpx` and `pnx`).
 *
 * All three forms hand over to the pnpm beside them rather than to whatever PATH
 * names first, so another pnpm earlier on PATH cannot take over the call.
 *
 * The sibling they reach is the bin `pnpm add -g` linked for the CLI this command
 * just installed: a pnpm / pnpm.cmd / pnpm.ps1 shim trio, one per shell. The bin
 * linker writes a bare pnpm.exe only for the `node` bin name, so each form has
 * exactly one sibling to name.
 */
function createShellScript (targetDir: string, name: string, subcommand: string): void {
  // windows can also use shell script via mingw or cygwin so no filter
  const shellScript = `#!/bin/sh
# $0 is whatever shim or symlink \`${name}\` was launched through, so walk to the
# file itself before looking beside it. The hop cap matches the kernel's ELOOP
# limit, so a cycle cannot hang the script. Directories come from \`\${self%/*}\`
# and \`readlink\` runs through \`command -p\`, so the caller's PATH decides nothing here.
self=$0
# \`\${self%/*}\` needs a slash to strip. A bare name came from a PATH lookup and
# stands for a file in the current directory.
case $self in
  */*) ;;
  *) self=./$self ;;
esac
hops=0
while [ -L "$self" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops + 1))
  link=$(command -p readlink "$self")
  case $link in
    /*) self=$link ;;
    *) self=\${self%/*}/$link ;;
  esac
done
# The walk has to end at a regular file. Running out of hops leaves $self a
# symlink; a chain that changed under us can leave it dangling or a directory, and
# a failed readlink leaves a trailing slash. Each case would take \`pnpm\` from the
# wrong directory — the substitution this script exists to prevent.
if [ -L "$self" ] || [ ! -f "$self" ]; then
  echo "${name}: could not resolve $0 to a regular file within 40 symlink hops." >&2
  exit 1
fi

exec "\${self%/*}/pnpm"${subcommand} "$@"
`
  fs.writeFileSync(path.join(targetDir, name), shellScript, { mode: 0o755 })

  if (process.platform === 'win32') {
    // `call`, so control comes back and this script's exit code is the shim's.
    // `%~dp0` already ends in a backslash.
    fs.writeFileSync(path.join(targetDir, `${name}.cmd`), `@echo off\r\ncall "%~dp0pnpm.cmd"${subcommand} %*\r\n`)
    // Also pnpm.cmd, not pnpm.ps1: the bin linker omits the PowerShell shim for a
    // package named `pnpm` (makePowerShellShim), so the sibling .ps1 may not exist
    // while the .cmd always does. $basedir is spelled the way the generated .ps1
    // shims spell it, so this works on PowerShell 2.0 as well.
    fs.writeFileSync(path.join(targetDir, `${name}.ps1`), `$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n& "$basedir\\pnpm.cmd"${subcommand} @args\nexit $LastExitCode\n`)
  }
}

// v10-layout shim names that v11 writes under pnpmHomeDir/bin instead.
export const LEGACY_HOME_DIR_SHIM_NAMES = [
  'pnpm', 'pnpm.cmd', 'pnpm.ps1',
  'pn', 'pn.cmd', 'pn.ps1',
  'pnpx', 'pnpx.cmd', 'pnpx.ps1',
  'pnx', 'pnx.cmd', 'pnx.ps1',
]

function removeLegacyHomeDirShims (pnpmHomeDir: string): void {
  for (const name of LEGACY_HOME_DIR_SHIM_NAMES) {
    try {
      fs.rmSync(path.join(pnpmHomeDir, name), { force: true })
    } catch {}
  }
}

export async function handler (
  opts: {
    force?: boolean
    pnpmHomeDir: string
  }
): Promise<string> {
  const execPath = getExecPath()
  const binDir = path.join(opts.pnpmHomeDir, 'bin')
  validateGHActionsEnvFileValues(opts.pnpmHomeDir, binDir)
  if (execPath.match(/\.[cm]?js$/) == null) {
    installCliGlobally(execPath, opts.pnpmHomeDir)
    createAliasScripts(binDir)
  }
  try {
    const report = await addDirToEnvPath(opts.pnpmHomeDir, {
      configSectionName: 'pnpm',
      proxyVarName: 'PNPM_HOME',
      proxyVarSubDir: 'bin',
      overwrite: opts.force,
      position: 'start',
    })
    writeGHActionsEnvFiles(opts.pnpmHomeDir, binDir)
    removeLegacyHomeDirShims(opts.pnpmHomeDir)
    return renderSetupOutput(report)
  } catch (err: any) { // eslint-disable-line
    switch (err.code) {
      case 'ERR_PNPM_BAD_ENV_FOUND':
        err.hint = 'If you want to override the existing env variable, use the --force option'
        break
      case 'ERR_PNPM_BAD_SHELL_SECTION':
        err.hint = 'If you want to override the existing configuration section, use the --force option'
        break
    }
    throw err
  }
}

function renderSetupOutput (report: PathExtenderReport): string {
  if (report.oldSettings === report.newSettings) {
    return 'No changes to the environment were made. Everything is already up to date.'
  }
  const output = []
  if (report.configFile) {
    output.push(reportConfigChange(report.configFile))
  }
  output.push(`Next configuration changes were made:
${report.newSettings}`)
  if (report.configFile == null) {
    output.push('Setup complete. Open a new terminal to start using pnpm.')
  } else if (report.configFile.changeType !== 'skipped') {
    output.push(`To start using pnpm, run:
source ${report.configFile.path}
`)
  }
  return output.join('\n\n')
}

function reportConfigChange (configReport: ConfigReport): string {
  switch (configReport.changeType) {
    case 'created': return `Created ${configReport.path}`
    case 'appended': return `Appended new lines to ${configReport.path}`
    case 'modified': return `Replaced configuration in ${configReport.path}`
    case 'skipped': return `Configuration already up to date in ${configReport.path}`
  }
}
