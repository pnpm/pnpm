import assert from 'node:assert/strict'
import console from 'node:console'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import { registerHooks } from 'node:module'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

const repo = process.argv[2]
const loaded = new Set()
if (process.env.PNPM_LOADER_AUDIT) {
  registerHooks({
    load (url, context, nextLoad) {
      const result = nextLoad(url, context)
      if (url.includes('/.pnpm-loader/')) loaded.add(url)
      return result
    },
  })
  process.on('exit', () => fs.writeFileSync(process.env.PNPM_LOADER_AUDIT, JSON.stringify([...loaded])))
}
const { compile } = await import(pathToFileURL(path.join(repo, 'packages/svelte/src/compiler/index.js')))
const directory = 'packages/svelte/tests/runtime-runes/samples'
const names = fs.readdirSync(path.join(repo, directory)).sort()
  .filter(name => fs.existsSync(path.join(repo, directory, name, 'main.svelte'))).slice(0, 100)
assert.equal(names.length, 100)
const outputs = []
for (const name of names) {
  for (const generate of ['client', 'server']) {
    const filename = `${directory}/${name}/main.svelte`
    const output = compile(fs.readFileSync(path.join(repo, filename), 'utf8'), { filename, generate, experimental: { async: true } })
    const hash = createHash('sha256').update(JSON.stringify({ js: output.js, css: output.css, warnings: output.warnings })).digest('hex')
    outputs.push({ filename, generate, hash })
  }
}
console.log(JSON.stringify(outputs))
