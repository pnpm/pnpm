import getNpmTarballUrl from 'get-npm-tarball-url'

export function getTarballUrl (pkgName: string, pkgVersion: string, registry: string): string {
  if (pkgName.startsWith('@jsr/')) return getJsrTarballUrl(pkgName, pkgVersion, registry)
  return getNpmTarballUrl(pkgName, pkgVersion, { registry })
}

function getJsrTarballUrl (pkgName: string, pkgVersion: string, registry: string): string {
  return `${registry}~/11/${pkgName}/${pkgVersion}.tgz`
}
