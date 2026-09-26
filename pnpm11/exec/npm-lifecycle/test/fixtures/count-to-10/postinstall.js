'use strict'

console.log('line 1')
console.log('line 2')
console.error('some error')
if (process.env.npm_package_json) {
  console.log('package.json')
}
