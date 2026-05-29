#! /usr/bin/env node
const fs = require('fs')
const file = require('./file')
const json = fs.readFileSync(file.FULL_PATH, 'utf-8')
console.log(json)
