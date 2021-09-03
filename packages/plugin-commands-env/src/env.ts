import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import PnpmError from '@pnpm/error'
import fetch from '@pnpm/fetch'
import cmdShim from '@zkochan/cmd-shim'
import renderHelp from 'render-help'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'
import { getNodeDir, NvmNodeCommandOptions } from './node'

export function rcOptionsTypes () {
  return {}
}

export function cliOptionsTypes () {
  return {
    global: Boolean,
  }
}

export const commandNames = ['env']

export function help () {
  return renderHelp({
    description: 'Install and use the specified version of Node.js. The npm CLI bundled with the given Node.js version gets installed as well.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Installs Node.js globally',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('env'),
    usages: [
      'pnpm env use --global <version>',
      'pnpm env use --global 16',
      'pnpm env use --global lts',
      'pnpm env use --global argon',
    ],
  })
}

export async function handler (opts: NvmNodeCommandOptions, params: string[]) {
  if (params.length === 0) {
    throw new PnpmError('ENV_NO_SUBCOMMAND', 'Please specify the subcommand')
  }
  switch (params[0]) {
  case 'use': {
    if (!opts.global) {
      throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
    }
    const nodeVersion = await resolveNodeVersion(params[1])
    if (!nodeVersion) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[1]}`)
    }
    const nodeDir = await getNodeDir({
      ...opts,
      useNodeVersion: nodeVersion,
    })
    const src = path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
    const dest = path.join(opts.bin, 'node')
    const cmdShimOpts = { createPwshFile: false }
    await cmdShim(src, dest, cmdShimOpts)
    try {
      let npmDir = nodeDir
      if (process.platform !== 'win32') {
        npmDir = path.join(npmDir, 'lib')
      }
      npmDir = path.join(npmDir, 'node_modules/npm/bin')
      await cmdShim(path.join(npmDir, 'npm-cli.js'), path.join(opts.bin, 'npm'), cmdShimOpts)
      await cmdShim(path.join(npmDir, 'npx-cli.js'), path.join(opts.bin, 'npx'), cmdShimOpts)
    } catch (err) {
      // ignore
    }
    return `Node.js ${nodeVersion} is activated
  ${dest} -> ${src}`
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}

interface NodeVersion {
  version: string
  lts: false | string
}

async function resolveNodeVersion (rawVersionSelector: string) {
  const response = await fetch('https://nodejs.org/download/release/index.json')
  const allVersions = (await response.json()) as NodeVersion[]
  const { versions, versionSelector } = filterVersions(allVersions, rawVersionSelector)
  const pickedVersion = semver.maxSatisfying(versions.map(({ version }) => version), versionSelector)
  if (!pickedVersion) return null
  return pickedVersion.substring(1)
}

function filterVersions (versions: NodeVersion[], versionSelector: string) {
  if (versionSelector === 'lts') {
    return {
      versions: versions.filter(({ lts }) => lts !== false),
      versionSelector: '*',
    }
  }
  const vst = versionSelectorType(versionSelector)
  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()
    return {
      versions: versions.filter(({ lts }) => typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion),
      versionSelector: '*',
    }
  }
  return { versions, versionSelector }
}
