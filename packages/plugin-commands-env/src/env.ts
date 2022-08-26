import { promises as fs, existsSync } from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import PnpmError from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersion } from '@pnpm/node.resolver'
import { globalInfo } from '@pnpm/logger'
import cmdShim from '@zkochan/cmd-shim'
import renderHelp from 'render-help'
import { getNodeDir, NvmNodeCommandOptions, getNodeVersionsBaseDir } from './node'
import getNodeMirror from './getNodeMirror'
import { parseNodeEditionSpecifier } from './parseNodeEditionSpecifier'

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
            description: 'Installs and uninstalls Node.js globally',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('env'),
    usages: [
      'pnpm env [command] [options] <version>',
      'pnpm env use --global 16',
      'pnpm env use --global lts',
      'pnpm env use --global argon',
      'pnpm env use --global latest',
      'pnpm env use --global rc/16',
      'pnpm env uninstall --global 16',
      'pnpm env uninstall --global lts',
      'pnpm env uninstall --global argon',
      'pnpm env uninstall --global latest',
      'pnpm env uninstall --global rc/16',
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
    const fetch = createFetchFromRegistry(opts)
    const { releaseChannel, versionSpecifier } = parseNodeEditionSpecifier(params[1])
    const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
    const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
    if (!nodeVersion) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[1]}`)
    }
    const nodeDir = await getNodeDir(fetch, {
      ...opts,
      useNodeVersion: nodeVersion,
      nodeMirrorBaseUrl,
    })
    const src = path.join(nodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
    const dest = path.join(opts.bin, process.platform === 'win32' ? 'node.exe' : 'node')
    try {
      await fs.unlink(dest)
    } catch (err) {}
    await fs.symlink(src, dest, 'file')
    try {
      let npmDir = nodeDir
      if (process.platform !== 'win32') {
        npmDir = path.join(npmDir, 'lib')
      }
      npmDir = path.join(npmDir, 'node_modules/npm')
      if (opts.configDir) {
        // We want the global npm settings to persist when Node.js or/and npm is changed to a different version,
        // so we tell npm to read the global config from centralized place that is outside of npm's directory.
        await fs.writeFile(path.join(npmDir, 'npmrc'), `globalconfig=${path.join(opts.configDir, 'npmrc')}`, 'utf-8')
      }
      const npmBinDir = path.join(npmDir, 'bin')
      const cmdShimOpts = { createPwshFile: false }
      await cmdShim(path.join(npmBinDir, 'npm-cli.js'), path.join(opts.bin, 'npm'), cmdShimOpts)
      await cmdShim(path.join(npmBinDir, 'npx-cli.js'), path.join(opts.bin, 'npx'), cmdShimOpts)
    } catch (err: any) { // eslint-disable-line
      // ignore
    }
    return `Node.js ${nodeVersion as string} is activated
  ${dest} -> ${src}`
  }
  case 'uninstall': {
    if (!opts.global) {
      throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
    }

    const fetch = createFetchFromRegistry(opts)
    const { releaseChannel, versionSpecifier } = parseNodeEditionSpecifier(params[1])
    const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
    const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
    const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

    if (!nodeVersion) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[1]}`)
    }

    const versionDir = path.resolve(nodeDir, nodeVersion)

    if (!existsSync(versionDir)) {
      throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${versionDir}`)
    }

    const nodePath = path.resolve(opts.pnpmHomeDir, process.platform === 'win32' ? 'node.exe' : 'node')
    const nodeLink = await fs.readlink(nodePath).catch(() => '')

    if (nodeLink.includes(versionDir)) {
      globalInfo(`Node.JS version ${nodeVersion} was detected as the default one, uninstalling ...`)

      const npmPath = path.resolve(opts.pnpmHomeDir, 'npm')
      const npxPath = path.resolve(opts.pnpmHomeDir, 'npx')

      await Promise.all([
        fs.unlink(nodePath),
        fs.unlink(npmPath),
        fs.unlink(npxPath),
      ])
    }

    const [processMajorVersion, processMinorVersion] = process.versions.node.split('.')

    /**
     * Support for earlier Node.JS versions
     * `fs.rmdir` is deprecated and `fs.rm` was added in v14.14.0
     * @see https://nodejs.org/dist/latest-v16.x/docs/api/fs.html#fsrmpath-options-callback
     */
    if (Number(processMajorVersion) === 14 && Number(processMinorVersion) <= 13) {
      await fs.rmdir(versionDir, { recursive: true })
    } else {
      await fs.rm(versionDir, { recursive: true })
    }

    return `Node.js ${nodeVersion} is uninstalled
  ${versionDir}`
  }
  default: {
    throw new PnpmError('ENV_UNKNOWN_SUBCOMMAND', 'This subcommand is not known')
  }
  }
}
