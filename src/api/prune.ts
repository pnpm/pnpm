import path = require('path')
import initCmd, {CommandNamespace} from './initCmd'
import {PnpmOptions, Package} from '../types'
import extendOptions from './extendOptions'
import {uninstallInContext} from './uninstall'
import getPkgDirs from '../fs/getPkgDirs'
import requireJson from '../fs/requireJson'
import lock from './lock'

export async function prune(maybeOpts?: PnpmOptions): Promise<void> {
  const opts = extendOptions(maybeOpts)

  const cmd: CommandNamespace = await initCmd(opts)

  return lock(cmd.ctx.store, async function () {
    if (!cmd.pkg) {
      throw new Error('No package.json found - cannot prune')
    }

    const pkg = cmd.pkg.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, cmd.ctx.root, opts.production)

    await uninstallInContext(extraneousPkgs, cmd.pkg, cmd, opts)
  })
}

export async function prunePkgs(pkgsToPrune: string[], maybeOpts?: PnpmOptions): Promise<void> {
  const opts = extendOptions(maybeOpts)

  const cmd: CommandNamespace = await initCmd(opts)

  return lock(cmd.ctx.store, async function () {
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
  })
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