import fetch, {PackageContentInfo, FetchedPackage} from './fetch'
import {PackageSpec, DirectoryResolution, Resolution, PackageMeta} from './resolve'
import {Store, read, save} from './fs/storeController'
import getRegistryName = require('encode-registry') // TODO: remove this. BREAKING CHANGE
import createGot, {Got} from './network/got'
import pkgIsUntouched from './pkgIsUntouched'
import pkgIdToFilename from './fs/pkgIdToFilename'

export {
  pkgIdToFilename,
  fetch,
  PackageContentInfo,
  FetchedPackage,
  PackageSpec,
  DirectoryResolution,
  Resolution,
  PackageMeta,
  Store,
  read,
  save,
  getRegistryName,
  createGot,
  Got,
  pkgIsUntouched,
}
