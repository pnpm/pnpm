'use strict'
const resolveFromLocal = require('@pnpm/local-resolver').default

resolveFromLocal({pref: './example-package'}, {prefix: process.cwd()})
  .then(resolveResult => console.log(resolveResult))
