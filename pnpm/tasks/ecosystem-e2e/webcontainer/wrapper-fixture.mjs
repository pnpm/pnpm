import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export function unpackWrapper (archive) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-webcontainer-wrapper-'))
  try {
    execFileSync('tar', ['-xzf', archive, '-C', directory])
    const files = new Map([['pnpm-wrapper.tgz', archive]])
    addFiles(files, directory, 'corepack')
    return { files, close: () => fs.rmSync(directory, { recursive: true, force: true }) }
  } catch (error) {
    fs.rmSync(directory, { recursive: true, force: true })
    throw error
  }
}

function addFiles (files, directory, prefix) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const source = path.join(directory, entry.name)
    const destination = `${prefix}/${entry.name}`
    if (entry.isDirectory()) addFiles(files, source, destination)
    else files.set(destination, source)
  }
}
