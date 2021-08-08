#!/bin/sh

download() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "$1"
    else
        wget -qO- "$1"
    fi
}

PATH="$(pwd)/buildtime-bin:$PATH"
export PATH
mkdir -p buildtime-bin
download https://get.pnpm.io/v6.7.js > buildtime-bin/v6.7.js
cat <<EOS > buildtime-bin/pnpm
#!/bin/sh
node $(pwd)/buildtime-bin/v6.7.js "\$@"
exit "\$?"
EOS
chmod 0755 buildtime-bin/pnpm
pnpm install
pnpm run compile-only
pnpm run copy-artifacts
