import { URL } from 'node:url'

import { registerStoreLoader } from './index.mjs'

registerStoreLoader(new URL('./.pnpm-store.json', import.meta.url))
