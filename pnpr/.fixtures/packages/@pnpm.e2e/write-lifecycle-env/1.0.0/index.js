#!/usr/bin/env node
'use strict'
const fs = require('fs')
const path = require('path')

const content = JSON.stringify(process.env, null, 2)
const fn = path.resolve('env.json')

console.log(`Writing ${fn}...`)

fs.writeFileSync(fn, content, 'utf8')

console.log(`Writing ${fn}, done`)
