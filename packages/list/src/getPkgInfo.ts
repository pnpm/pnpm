import path = require('path')
import readPkg from './readPkg'

export default async function getPkgInfo (
  pkg: {
    name: string,
    version: string,
    path: string,
  },
) {
  const manifest = await readPkg(path.join(pkg.path, 'node_modules', pkg.name, 'package.json'))
  return {
    ...pkg,
    description: manifest.description,
    homepage: manifest.homepage,
    repository: manifest.repository && (
      typeof manifest.repository === 'string' ? manifest.repository : manifest.repository.url
    ) || undefined,
  }
}
