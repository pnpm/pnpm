const path = require('path')
const findPkgs = require('@pnpm/fs.find-packages')

findPkgs(path.join(import.meta.dirname, 'test/fixtures/one-pkg'))
  .then(pkgs => console.log(pkgs))
  .catch(err => console.error(err))
