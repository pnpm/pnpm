import logger from '@pnpm/logger'
import driveByPath = require('drive-by-path')
import osHomedir = require('os-homedir')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')

const STORE_VERSION = '2'
const STORE_DEFAULT_PATH = '~/.pnpm-store'

export default async function (
  storePath: string | undefined,
  pkgRoot: string,
) {
  const pkgDrive = await safeDriveByPath(pkgRoot)

  if (!storePath || isHomepath(storePath)) {
    const relStorePath = storePath ? storePath.substr(2) : '.pnpm-store'
    return await storePathRelativeToHome(pkgDrive, relStorePath)
  }

  const storeBasePath = pathAbsolute(storePath, pkgRoot)

  const storeDrive = await safeDriveByPath(storeBasePath)

  if (pkgDrive && storeDrive && pkgDrive.device !== storeDrive.device) {
    logger.warn('The store is not on the same device on which the installation is done. ' +
      `Store is on ${storeDrive.displayName}, device is on ${pkgDrive.displayName}`)
  }

  if (storeBasePath.endsWith(`${path.sep}${STORE_VERSION}`)) {
    return storeBasePath
  }
  return path.join(storeBasePath, STORE_VERSION)
}

function shortestDriveMountpoint (
  drive: {
    mountpoints: Array<{
      path: string,
    }>,
  },
): string {
  // `as string` might be a bad thing to do but as of now,
  // this will never return undefined
  return R.head((R.map(R.prop('path'), drive.mountpoints) as string[]).sort()) as string
}

async function storePathRelativeToHome (pkgDrive: driveByPath.Drive | null, relStore: string) {
  const homedir = getHomedir()
  const homeDrive = await safeDriveByPath(homedir)
  if (!pkgDrive || !homeDrive || pkgDrive.device === homeDrive.device) {
    // If the project is on the drive on which the OS home directory
    // then the store is placed in the home directory
    return path.join(homedir, relStore, STORE_VERSION)
  }
  const mountpoint = shortestDriveMountpoint(pkgDrive)
  return path.join(mountpoint, relStore, STORE_VERSION)
}

function getHomedir () {
  const home = osHomedir()
  if (!home) throw new Error('Could not find the homedir')
  return home
}

async function safeDriveByPath (p: string) {
  try {
    return await driveByPath(p)
  } catch (err) {
    // When devices could not be detected
    // it is assumed that the project is on the same device as the store
    return null
  }
}

function isHomepath (filepath: string) {
  return filepath.indexOf('~/') === 0 || filepath.indexOf('~\\') === 0
}
