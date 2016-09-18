import {Package} from './api/initCmd'

/**
 * Returns bins for a package in a standard object format. This normalizes
 * between npm's string and object formats.
 *
 * @example
 *    binify({ name: 'rimraf', bin: 'cmd.js' })
 *    => { rimraf: 'cmd.js' }
 *
 *    binify({ name: 'rmrf', bin: { rmrf: 'cmd.js' } })
 *    => { rmrf: 'cmd.js' }
 */
export default function binify (pkg: Package) {
  if (typeof pkg.bin === 'string') {
    const obj = {}
    obj[pkg.name] = pkg.bin
    return obj
  }

  return pkg.bin || {}
}
