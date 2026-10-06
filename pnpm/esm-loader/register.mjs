import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { registerStoreLoader } from './index.mjs'

registerStoreLoader(pathToFileURL(path.resolve(process.env.PNPM_LOADER_MANIFEST ?? 'node_modules/.pnpm/.store-manifest.json')))
