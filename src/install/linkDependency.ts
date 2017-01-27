import path = require('path')
import linkDir from 'link-dir'
import logger from 'pnpm-logger'
import {InstalledPackage} from '../types'

export default async function linkDependency (dep: InstalledPackage, toPackage: InstalledPackage) {
  await linkDir(
    dep.hardlinkedLocation,
    path.join(toPackage.hardlinkedLocation, '..', 'node_modules', dep.pkg.name)
  )
}
