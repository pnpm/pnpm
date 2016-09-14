import {basename} from 'path'

export default (tarballPath: string) => basename(tarballPath).replace(/(\.tgz|\.tar\.gz)$/i, '')
