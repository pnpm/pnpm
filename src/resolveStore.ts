import pathAbsolute = require('path-absolute')
import path = require('path')

const STORE_VERSION = '2'

export default function (storePath: string | undefined, pkgRoot: string) {
  storePath = storePath || '~/.pnpm-store'
  const storeBasePath = pathAbsolute(storePath, pkgRoot)
  if (storeBasePath.endsWith(`${path.sep}${STORE_VERSION}`)) {
    return storeBasePath
  }
  return path.join(storeBasePath, STORE_VERSION)
}
