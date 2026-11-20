import { copyFileSync, lstatSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const directories = ['docs', 'pnpr-docs', 'versioned_docs/version-11.x', 'static/docs-assets']
const files = ['sidebars.json', 'sidebars-pnpr.json', 'versioned_sidebars/version-11.x-sidebars.json', 'docs-sync.json']
// Release pages are added next to the hand-written ones, so this directory is never cleared.
const releasePages = /^blog\/releases\/[^/]+\.md$/

export function copyDocsArtifact (source, destination) {
  const entries = []
  function visit (relative = '') {
    const full = path.join(source, relative)
    const stat = lstatSync(full)
    if (stat.isDirectory()) {
      for (const name of readdirSync(full)) visit(path.join(relative, name))
    } else {
      const parts = relative.split(path.sep)
      const normalized = parts.join('/')
      if (!stat.isFile() || parts.some(part => part.startsWith('.')) ||
          !(files.includes(normalized) || releasePages.test(normalized) || directories.some(dir => normalized.startsWith(`${dir}/`)))) {
        throw new Error(`Unexpected documentation artifact: ${relative}`)
      }
      entries.push(relative)
    }
  }
  visit()
  for (const file of files) {
    if (!entries.includes(file.split('/').join(path.sep))) throw new Error(`Missing documentation artifact: ${file}`)
  }
  for (const dir of directories.slice(0, 3)) {
    if (!entries.some(file => file.startsWith(`${dir.split('/').join(path.sep)}${path.sep}`))) throw new Error(`Missing documentation artifact: ${dir}`)
  }
  for (const entry of [...directories, ...files]) rmSync(path.join(destination, entry), { recursive: true, force: true })
  for (const relative of entries) {
    const target = path.join(destination, relative)
    mkdirSync(path.dirname(target), { recursive: true })
    copyFileSync(path.join(source, relative), target)
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [source, destination] = process.argv.slice(2)
  if (!source || !destination) throw new Error('Usage: copy-docs-artifact.mjs SOURCE DESTINATION')
  copyDocsArtifact(source, destination)
}
