import path = require('path')
import writeFileAtomicCB = require('write-file-atomic')
import thenify = require('thenify')
import rimraf = require('rimraf-then')
import yaml = require('js-yaml')
import {SHRINKWRAP_FILENAME, PRIVATE_SHRINKWRAP_FILENAME} from './constants'
import {Shrinkwrap} from './types'

const writeFileAtomic = thenify(writeFileAtomicCB)

const SHRINKWRAP_YAML_FORMAT = {
  sortKeys: true,
  lineWidth: 1000,
  noCompatMode: true,
}

export default function write (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  const privateShrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(shrinkwrap.specifiers).length === 0) {
    return Promise.all([
      rimraf(shrinkwrapPath),
      rimraf(privateShrinkwrapPath),
    ])
  }

  const yamlDoc = yaml.safeDump(shrinkwrap, SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(shrinkwrapPath, yamlDoc),
    writeFileAtomic(privateShrinkwrapPath, yamlDoc),
  ])
}
