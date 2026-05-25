#!/usr/bin/env node
const fs = require('fs')
const out = process.argv[2]
let input = ''
process.stdin.on('data', chunk => input += chunk)
process.stdin.on('end', () => {
  let data = []
  if (fs.existsSync(out)) data = JSON.parse(fs.readFileSync(out, 'utf8'))
  data.push(Number(input))
  fs.writeFileSync(out, JSON.stringify(data))
})
