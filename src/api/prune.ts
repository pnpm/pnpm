import path = require('path')
import initCmd, {CommandNamespace} from './initCmd'
import {PublicInstallationOptions, StrictPublicInstallationOptions} from './install'
import defaults from '../defaults'
import {uninstallInContext} from './uninstall'
import getPkgDirs from '../fs/getPkgDirs'
import requireJson from '../fs/requireJson'

export default prune

async function prune(optsNullable: PublicInstallationOptions): Promise<void>
async function prune(pkgsToPrune: string[], optsNullable: PublicInstallationOptions): Promise<void>
async function prune() {
  let pkgsToPrune: string[] | void
  let optsNullable: PublicInstallationOptions
  if (arguments.length === 1) {
    pkgsToPrune = undefined
    optsNullable = arguments[0]
  } else {
    pkgsToPrune = arguments[0]
    optsNullable = arguments[1]
    if (pkgsToPrune.length === 0) {
      throw new Error('You have to be specify at least one package to prune or use the prune all overload')
    }
  }
  const opts: StrictPublicInstallationOptions = Object.assign({}, defaults, optsNullable)

  const cmd: CommandNamespace = await initCmd(opts)

  try {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot prune')
    }
    const pkg = cmd.pkg.pkg

    const saveTypes = getSaveTypes(opts.production)
    const savedDepsMap = saveTypes.reduce((allDeps, deps) => Object.assign({}, allDeps, pkg[deps]), {})
    const savedDeps = Object.keys(savedDepsMap)
    const modules = path.join(cmd.ctx.root, 'node_modules')
    const pkgsInFS = await getPkgsInFS(modules)
    const extraneousPkgs = pkgsInFS.filter((pkgInFS: string) => savedDeps.indexOf(pkgInFS) === -1)

    let pkgsToUninstall: string[]
    if (pkgsToPrune && pkgsToPrune.length) {
      const notPrunable = pkgsToPrune.filter(pkgToPrune => extraneousPkgs.indexOf(pkgToPrune) === -1)
      if (notPrunable.length) {
        const err = new Error(`Unable to prune ${notPrunable.join(', ')} because it is not an extraneous package`)
        err['code'] = 'PRUNE_NOT_EXTR'
        throw err
      }
      pkgsToUninstall = pkgsToPrune
    } else {
      pkgsToUninstall = extraneousPkgs
    }
    await uninstallInContext(pkgsToUninstall, cmd.pkg, cmd, opts)

    await cmd.unlock()
  } catch (err) {
    if (typeof cmd !== 'undefined' && cmd.unlock) await cmd.unlock()
    throw err
  }
}

const prodDepTypes = ['dependencies', 'optionalDependencies']
const devOnlyDepTypes = ['devDependencies']

function getSaveTypes (production: boolean) {
  if (production) {
    return prodDepTypes
  }
  return prodDepTypes.concat(devOnlyDepTypes)
}

async function getPkgsInFS (modules: string): Promise<string[]> {
  const pkgDirs = await getPkgDirs(modules)
  return pkgDirs.map((pkgDirPath: string) => {
    const pkgJsonPath = path.join(pkgDirPath, 'package.json')
    const pkgJSON = requireJson(pkgJsonPath)
    return pkgJSON.name
  })
}