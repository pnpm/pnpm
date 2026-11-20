const fs = require('fs')
fs.rmSync('should-be-deleted-by-build1.txt', { force: true })
fs.writeFileSync('should-be-modified-by-build1.txt', 'After modification')
fs.writeFileSync('should-be-added-by-build1.txt', __filename)
