import registryUrl = require('registry-url')

/*
 * Returns the registry needed for a certain package.
 */

export default function registryFor (pkg: string) {
  if (pkg.substr(0, 1) === '@') {
    return registryUrl(pkg.split('/')[0])
  } else {
    return registryUrl()
  }
}
