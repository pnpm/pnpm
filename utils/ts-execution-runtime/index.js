const babelRegister = require('@babel/register')
const path = require('path')

const root = path.dirname(path.dirname(__dirname))

babelRegister({
  root,
  cwd: root,
  extensions: ['.ts'],
  presets: [
    '@babel/preset-typescript',
  ],
  plugins: [
    '@babel/plugin-transform-modules-commonjs',
    '@babel/plugin-proposal-dynamic-import',
    require.resolve('./rewrite-imports.js')
  ]
})
