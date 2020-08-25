import readPkg from './readPkg'
import path = require('path')

export default async function getPkgInfo (
  pkg: {
    alias: string
    name: string
    version: string
    path: string
    resolved?: string
  }
) {
  let manifest
  try {
    manifest = await readPkg(path.join(pkg.path, 'node_modules', pkg.name, 'package.json'))
  } catch (err) {
    // This will probably never happen
    manifest = {
      description: '[Could not find additional info about this dependency]',
    }
  }
  return {
    alias: pkg.alias,
    from: pkg.name,

    version: pkg.version,

    resolved: pkg.resolved,

    description: manifest.description,

    homepage: manifest.homepage,
    repository: (manifest.repository && (
      typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
    )) ?? undefined,
  }
}
