'use strict'

// Prototype loader: locates the compiled native addon for the current
// platform/arch and re-exports its functions. Real napi-rs packages ship
// prebuilds per triple and have a more elaborate loader; that's out of
// scope for a prototype.

const { existsSync } = require('node:fs')
const { join } = require('node:path')

const triple = `${process.platform}-${process.arch}`
const candidate = join(__dirname, `pnpm-cafs-writer.${triple}.node`)

if (!existsSync(candidate)) {
  throw new Error(
    `@pnpm/agent.cafs-writer: native addon not built for ${triple}. ` +
    `Run \`pnpm --filter @pnpm/agent.cafs-writer run build\` first.`
  )
}

module.exports = require(candidate)
