import dirsum = require('@zkochan/dirsum')

export default function (dirpath: string): Promise<string> {
  return new Promise((resolve, reject) => {
    dirsum.digest(dirpath, 'sha1', (err: Error, hashes: {hash: string}) => {
      if (err) return reject(err)
      resolve(hashes.hash)
    })
  })
}
