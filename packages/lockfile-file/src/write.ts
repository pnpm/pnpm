import {
  CURRENT_LOCKFILE,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { Shrinkwrap } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import yaml = require('js-yaml')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import promisify = require('util.promisify')
import writeFileAtomicCB = require('write-file-atomic')
import logger from './logger'

const writeFileAtomic = promisify(writeFileAtomicCB)

const SHRINKWRAP_YAML_FORMAT = {
  lineWidth: 1000,
  noCompatMode: true,
  noRefs: true,
  sortKeys: true,
}

export function writeWantedOnly (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  return writeShrinkwrap(WANTED_LOCKFILE, pkgPath, wantedShrinkwrap, opts)
}

export async function writeCurrentOnly (
  pkgPath: string,
  currentShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  await mkdirp(path.join(pkgPath, 'node_modules'))
  return writeShrinkwrap(CURRENT_LOCKFILE, pkgPath, currentShrinkwrap, opts)
}

function writeShrinkwrap (
  shrinkwrapFilename: string,
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const shrinkwrapPath = path.join(pkgPath, shrinkwrapFilename)

  // empty shrinkwrap is not saved
  if (isEmptyShrinkwrap(wantedShrinkwrap)) {
    return rimraf(shrinkwrapPath)
  }

  const yamlDoc = yaml.safeDump(normalizeShrinkwrap(wantedShrinkwrap, opts && opts.forceSharedFormat === true || false), SHRINKWRAP_YAML_FORMAT)

  return writeFileAtomic(shrinkwrapPath, yamlDoc)
}

function isEmptyShrinkwrap (shr: Shrinkwrap) {
  return R.values(shr.importers).every((importer) => R.isEmpty(importer.specifiers || {}) && R.isEmpty(importer.dependencies || {}))
}

function normalizeShrinkwrap (shr: Shrinkwrap, forceSharedFormat: boolean) {
  if (forceSharedFormat === false && R.equals(R.keys(shr.importers), ['.'])) {
    const shrToSave = {
      ...shr,
      ...shr.importers['.'],
    }
    delete shrToSave.importers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (R.isEmpty(shrToSave[depType])) {
        delete shrToSave[depType]
      }
    }
    if (R.isEmpty(shrToSave.packages)) {
      delete shrToSave.packages
    }
    return shrToSave
  } else {
    const shrToSave = {
      ...shr,
      importers: R.keys(shr.importers).reduce((acc, alias) => {
        const importer = shr.importers[alias]
        const normalizedImporter = {
          specifiers: importer.specifiers,
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!R.isEmpty(importer[depType] || {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (R.isEmpty(shrToSave.packages)) {
      delete shrToSave.packages
    }
    return shrToSave
  }
}

export default function write (
  pkgPath: string,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const wantedShrinkwrapPath = path.join(pkgPath, WANTED_LOCKFILE)
  const currentShrinkwrapPath = path.join(pkgPath, CURRENT_LOCKFILE)

  // empty shrinkwrap is not saved
  if (isEmptyShrinkwrap(wantedShrinkwrap)) {
    return Promise.all([
      rimraf(wantedShrinkwrapPath),
      rimraf(currentShrinkwrapPath),
    ])
  }

  const forceSharedFormat = opts && opts.forceSharedFormat === true || false
  const yamlDoc = yaml.safeDump(normalizeShrinkwrap(wantedShrinkwrap, forceSharedFormat), SHRINKWRAP_YAML_FORMAT)

  // in most cases the `shrinkwrap.yaml` and `node_modules/.shrinkwrap.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (wantedShrinkwrap === currentShrinkwrap) {
    return Promise.all([
      writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
      (async () => {
        await mkdirp(path.dirname(currentShrinkwrapPath))
        await writeFileAtomic(currentShrinkwrapPath, yamlDoc)
      })(),
    ])
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${CURRENT_LOCKFILE}\``,
    prefix: pkgPath,
  })

  const currentYamlDoc = yaml.safeDump(normalizeShrinkwrap(currentShrinkwrap, forceSharedFormat), SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(wantedShrinkwrapPath, yamlDoc),
    (async () => {
      await mkdirp(path.dirname(currentShrinkwrapPath))
      await writeFileAtomic(currentShrinkwrapPath, currentYamlDoc)
    })(),
  ])
}
