import fs from 'node:fs'
import { registerHooks } from 'node:module'
import process from 'node:process'

const loaded = new Set()
if (process.env.PNPM_LOADER_AUDIT) {
  registerHooks({
    load (url, context, nextLoad) {
      const result = nextLoad(url, context)
      if (url.includes('/.pnpm-loader/')) loaded.add(url)
      return result
    },
  })
  process.on('exit', () => fs.appendFileSync(process.env.PNPM_LOADER_AUDIT, JSON.stringify([...loaded]) + '\n'))
}
