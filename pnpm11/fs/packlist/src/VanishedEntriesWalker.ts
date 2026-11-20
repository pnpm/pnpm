import fs from 'node:fs'
import path from 'node:path'

import npmPacklist from 'npm-packlist'

// How many times a directory whose listing keeps failing with ENOENT is read.
const LISTING_ATTEMPTS = 8

/**
 * npm-packlist's walker, except that an entry another process removes while
 * the walk reads it is skipped, as it is not part of the package. A missing
 * package root still fails the walk.
 */
export class VanishedEntriesWalker extends npmPacklist.Walker {
  override start (): this {
    this.readdir(LISTING_ATTEMPTS)
    return this
  }

  // On macOS, listings of a directory that still exists have failed with
  // ENOENT while another process renamed one of its entries, so such a
  // listing is read again, and one that keeps failing is an error.
  private readdir (attemptsLeft: number): void {
    fs.readdir(this.path, (err, entries) => {
      if (err == null) {
        this.onReaddir(entries)
        return
      }
      if (err.code === 'ENOENT') {
        this.onListingNotFound(err, attemptsLeft)
      } else {
        this.emit('error', err)
      }
    })
  }

  private onListingNotFound (err: NodeJS.ErrnoException, attemptsLeft: number): void {
    fs.lstat(this.path, (statErr) => {
      if (statErr?.code === 'ENOENT' && this.parent != null) {
        this.onReaddir([])
      } else if (statErr == null && attemptsLeft > 1) {
        this.readdir(attemptsLeft - 1)
      } else {
        this.emit('error', err)
      }
    })
  }

  // ignore-walk's stat, as npm-packlist configures it: no symlinks followed,
  // and names with `*` skipped, which node-tar cannot pack on Windows.
  override stat (opts: Omit<npmPacklist.StatOptions, 'isSymbolicLink' | 'st'>, callback: () => void): void {
    if (opts.entry.includes('*')) {
      callback()
      return
    }
    fs.lstat(`${this.path}/${opts.entry}`, (err, st) => {
      if (err == null) {
        this.onstat({ ...opts, isSymbolicLink: st.isSymbolicLink(), st }, callback)
      } else if (err.code === 'ENOENT') {
        callback()
      } else {
        this.emit('error', err)
      }
    })
  }

  override addIgnoreFile (file: string, callback: () => void): void {
    if (file === 'package.json' && this.isPackage) {
      super.addIgnoreFile(file, callback)
      return
    }
    fs.readFile(path.resolve(this.path, file), 'utf8', (err, data) => {
      if (err == null) {
        this.onReadIgnoreFile(file, data, callback)
      } else if (err.code === 'ENOENT') {
        callback()
      } else {
        this.emit('error', err)
      }
    })
  }
}
