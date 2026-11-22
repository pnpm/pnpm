import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

export function auditEnvironment (root, filename) {
  fs.writeFileSync(filename, '')
  const preloads = ['loader.mjs', 'audit-cas.mjs'].map(name => '--import=' + pathToFileURL(path.join(root, name)).href)
  return { NODE_OPTIONS: preloads.join(' '), PNPM_LOADER_AUDIT: filename }
}

export function readAudit (filename) {
  const lines = fs.readFileSync(filename, 'utf8').trim()
  const loaded = new Set(lines ? lines.split('\n').flatMap(line => JSON.parse(line)) : [])
  return {
    casModules: loaded.size,
    casPackages: new Set([...loaded].map(url => url.split('/.pnpm-loader/')[1].split('/')[0])).size,
  }
}
