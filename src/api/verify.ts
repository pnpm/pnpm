import path = require('path')
import R = require('ramda')
import fs = require('fs')
import {Package, Dependencies} from '../types'
import {
  Shrinkwrap,
  SHRINKWRAP_FILENAME,
  PRIVATE_SHRINKWRAP_FILENAME,
} from '../fs/shrinkwrap'
import {PnpmError} from '../errorTypes'
import readPkg = require('read-pkg')
import streamEqual = require('stream-equal')
import readline = require('readline')
import yaml = require('js-yaml')

export default async function (pkgPath: string) {
  const privateShrPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)
  const privateShrStream = fs.createReadStream(privateShrPath, 'UTF8')

  const publicShrPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  const publicShrStream = fs.createReadStream(publicShrPath, 'UTF8')

  const isOutdatedNodeModules = new Promise((resolve, reject) => {
    streamEqual(privateShrStream, publicShrStream, (err: Error, equal: boolean) => {
      if (err) return reject(err)
      if (!equal) {
        return reject(new PnpmError('OUTDATED_NODE_MODULES', 'node_modules needs reinstallation'))
      }
      resolve()
    })
  })

  const shrReader = readline.createInterface({input: publicShrStream})

  const isOutdatedShrFile = Promise.all([
    new Promise((resolve, reject) => {
      let shrHead = ''

      shrReader.on('line', onLine)

      function onLine (line: string) {
        if (line === 'packages:') {
          shrReader.removeListener('line', onLine)
          resolve(shrHead)
          return
        }
        shrHead += `${line}\n`
      }
    }),
    readPkg(pkgPath)
  ])
  .then((results: {}) => {
    const parsedShr = yaml.safeLoad(results[0])

    if (!shrinkwrapIsUpToDate(parsedShr, results[1])) {
      throw new PnpmError('OUTDATED_SHRINKWRAP_FILE', 'shrinkwrap file in the project root is not up to date')
    }
  })

  try {
    await Promise.all([
      isOutdatedNodeModules,
      isOutdatedShrFile
    ])
  } catch (err) {
    return err
  }

  return null
}

function shrinkwrapIsUpToDate (shr: Shrinkwrap, pkg: Package) {
  const deps: Dependencies = Object.assign({}, pkg.devDependencies, pkg.optionalDependencies, pkg.dependencies)
  if (R.keys(deps).length !== R.keys(shr.dependencies).length) return false
  return R.equals(
    R.keys(deps).map(depName => `${depName}@${deps[depName]}`).sort(),
    R.keys(shr.dependencies).sort()
  )
}
