import install from './api/install'
import installPkgDeps from './api/installPkgDeps'
import uninstall from './api/uninstall'
export * from './api/link'
export * from './api/prune'

export {install, installPkgDeps, uninstall}

export {PnpmOptions} from './types'
