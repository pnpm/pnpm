import {basename} from 'path'

export default tarballPath => basename(tarballPath).replace(/(\.tgz|\.tar\.gz)$/i, '')
