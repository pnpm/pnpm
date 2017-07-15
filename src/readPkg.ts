import pLimit = require('p-limit')
import readPkgCB = require('read-package-json')
import thenify = require('thenify')

const limitPkgReads = pLimit(4)
const _readPkg = thenify(readPkgCB)
const readPkg = (pkgPath: string) => limitPkgReads(() => _readPkg(pkgPath))

export default readPkg
