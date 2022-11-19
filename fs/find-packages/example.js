const path = require('path')
const findPkgs = require('find-packages')

findPkgs(path.join(__dirname, 'test/fixtures/one-pkg'))
  .then(pkgs => console.log(pkgs))
  .catch(err => console.error(err))
