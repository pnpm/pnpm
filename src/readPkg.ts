import pLimit = require('p-limit')
import readPkgCB = require('read-package-json')
import thenify = require('thenify')

const limitPkgReads = pLimit(4)
const readPkgLib = thenify(readPkgCB)
const readPkg = (pkgPath: string) => limitPkgReads(() => readPkgLib(pkgPath))

export default readPkg
