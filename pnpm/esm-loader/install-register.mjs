import { URL } from 'node:url'

import { registerStoreLoader } from './index.mjs'

registerStoreLoader(new URL('./.store-manifest.json', import.meta.url))
