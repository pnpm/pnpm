'use strict'
const dependenciesHierarchy = require('../lib').default

dependenciesHierarchy(process.cwd(), {depth: 2})
  .then(tree => {
    console.log(tree)
  })
