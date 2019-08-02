import path = require('path')
import readPkg from './readPkg'

export default async function getPkgInfo (
  pkg: {
    alias: string,
    name: string,
    version: string,
    path: string,
    resolved?: string,
  },
) {
  const manifest = await readPkg(path.join(pkg.path, 'node_modules', pkg.name, 'package.json'))
  return {
    alias: pkg.alias,
    from: pkg.name,

    version: pkg.version,

    resolved: pkg.resolved,

    description: manifest.description,

    homepage: manifest.homepage,
    repository: manifest.repository && (
      typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
    ) || undefined,
  }
}
