import path = require('path')
import initCmd, {CommandNamespace, Package} from './initCmd'
import {PublicInstallationOptions, StrictPublicInstallationOptions} from './install'
import defaults from '../defaults'
import {uninstallInContext} from './uninstall'
import getPkgDirs from '../fs/getPkgDirs'
import requireJson from '../fs/requireJson'

export async function prune(optsNullable: PublicInstallationOptions): Promise<void> {
  const opts: StrictPublicInstallationOptions = Object.assign({}, defaults, optsNullable)

  const cmd: CommandNamespace = await initCmd(opts)

  try {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot prune')
    }
    const pkg = cmd.pkg.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, cmd.ctx.root, opts.production)

    await uninstallInContext(extraneousPkgs, cmd.pkg, cmd, opts)

    await cmd.unlock()
  } catch (err) {
    if (typeof cmd !== 'undefined' && cmd.unlock) await cmd.unlock()
    throw err
  }
}

export async function prunePkgs(pkgsToPrune: string[], optsNullable: PublicInstallationOptions): Promise<void> {
  const opts: StrictPublicInstallationOptions = Object.assign({}, defaults, optsNullable)

  const cmd: CommandNamespace = await initCmd(opts)

  try {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot prune')
    }
    const pkg = cmd.pkg.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, cmd.ctx.root, opts.production)

    const notPrunable = pkgsToPrune.filter(pkgToPrune => extraneousPkgs.indexOf(pkgToPrune) === -1)
    if (notPrunable.length) {
      const err = new Error(`Unable to prune ${notPrunable.join(', ')} because it is not an extraneous package`)
      err['code'] = 'PRUNE_NOT_EXTR'
      throw err
    }

    await uninstallInContext(pkgsToPrune, cmd.pkg, cmd, opts)

    await cmd.unlock()
  } catch (err) {
    if (typeof cmd !== 'undefined' && cmd.unlock) await cmd.unlock()
    throw err
  }
}

async function getExtraneousPkgs (pkg: Package, root: string, production: boolean) {
  const saveTypes = getSaveTypes(production)
  const savedDepsMap = saveTypes.reduce((allDeps, deps) => Object.assign({}, allDeps, pkg[deps]), {})
  const savedDeps = Object.keys(savedDepsMap)
  const modules = path.join(root, 'node_modules')
  const pkgsInFS = await getPkgsInFS(modules)
  const extraneousPkgs = pkgsInFS.filter((pkgInFS: string) => savedDeps.indexOf(pkgInFS) === -1)
  return extraneousPkgs
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