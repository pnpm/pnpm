'use strict'
const pnpmList = require('../lib').default

pnpmList(import.meta.dirname, {depth: 2})
  .then(output => {
    console.log(output)
  })
