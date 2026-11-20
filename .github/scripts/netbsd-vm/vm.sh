#!/usr/bin/env bash
# Builds release artifacts natively in a NetBSD VM that qemu runs with KVM.
#
# The `cross` NetBSD images cannot link pnpm: their NetBSD 9 sysroot has no
# libexecinfo (which Rust's std links) and no getentropy (which aws-lc calls).
# A native build on NetBSD 10 needs neither workaround.
#
# Every download is checked against a hash committed here: the NetBSD release
# files against release.sha512, anita and rustup-init against the constants
# below. The binaries built in the VM are attested and published.
#
# Usage:
#   vm.sh start          install NetBSD, boot it with this checkout attached, install Rust
#   vm.sh build <args>   run `cargo build <args>` in the VM and copy the
#                        top-level files of target/<triple>/<profile> back here
#   vm.sh run <command>  run a shell command in the VM

set -euo pipefail

NETBSD_RELEASE=10.2
ANITA_VERSION=2.18
ANITA_SHA256=fa5ac3a8b3e30a3a35df9254bbee213b28210a396544592c3e604a1e53a294ce
RUSTUP_VERSION=1.29.3
RUSTUP_SHA256=b3b81e3e737bc9465acfa5872c9c0afee7c6430bceaa52c038fbd9f3c327b51d

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(git -C "$script_dir" rev-parse --show-toplevel)
vm_dir=${RUNNER_TEMP:-/tmp}/netbsd-vm
ssh_port=2222
serial_port=4444

# The VM only listens on the runner's loopback and is created fresh by `start`,
# so there is no host key worth pinning.
vm_ssh() {
  ssh -i "$vm_dir/key" -p "$ssh_port" \
    -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR -o ServerAliveInterval=30 \
    root@127.0.0.1 "$@"
}

download() {
  # --retry alone does not retry a connection dropped mid-body (curl exit 18).
  curl --fail --silent --show-error --location \
    --retry 5 --retry-all-errors --retry-delay 5 --output "$2" "$1"
}

install_netbsd() {
  local release_dir="$vm_dir/release"
  local hash path
  # The release directory may be restored from the actions cache; a file
  # already there is downloaded again only if its hash does not match.
  while read -r hash path; do
    if (cd "$release_dir" 2>/dev/null && echo "$hash  $path" | sha512sum --check --status 2>/dev/null); then
      continue
    fi
    mkdir -p "$release_dir/$(dirname "$path")"
    download "https://cdn.netbsd.org/pub/NetBSD/NetBSD-$NETBSD_RELEASE/$path" "$release_dir/$path"
  done < "$script_dir/release.sha512"
  (cd "$release_dir" && sha512sum --check --quiet "$script_dir/release.sha512")

  download "https://www.gson.org/netbsd/anita/download/anita-$ANITA_VERSION.tar.gz" "$vm_dir/anita.tar.gz"
  echo "$ANITA_SHA256  $vm_dir/anita.tar.gz" | sha256sum --check --quiet
  tar xzf "$vm_dir/anita.tar.gz" -C "$vm_dir"

  PYTHONPATH="$vm_dir/anita-$ANITA_VERSION" python3 "$vm_dir/anita-$ANITA_VERSION/anita" \
    --workdir "$vm_dir/work" --disk-size 40G --memory-size 4G \
    --vmm-args "-enable-kvm -cpu host" \
    install "$release_dir/amd64/"
}

pack_source() {
  # `.pnpm/crates` links into the pnpm store, so it is packed with the links
  # followed: the VM builds offline from it.
  git -C "$repo_root" ls-files -z | tar -C "$repo_root" --null -T - -cf "$vm_dir/source.tar"
  tar -C "$repo_root" -rhf "$vm_dir/source.tar" .pnpm/crates
}

boot() {
  local memory_mb
  memory_mb=$(( $(awk '/^MemTotal:/ { print $2 }' /proc/meminfo) * 3 / 4 / 1024 ))
  # The source travels as a raw second disk, which the VM reads with tar.
  qemu-system-x86_64 -enable-kvm -cpu host -smp "$(nproc)" -m "${memory_mb}M" \
    -drive "file=$vm_dir/work/wd0.img,format=raw,media=disk" \
    -drive "file=$vm_dir/source.tar,format=raw,media=disk,snapshot=on" \
    -nic "user,model=e1000,hostfwd=tcp:127.0.0.1:$ssh_port-:22" \
    -display none -serial "tcp:127.0.0.1:$serial_port,server,nowait" -daemonize

  ssh-keygen -q -t ed25519 -N '' -f "$vm_dir/key"
  python3 "$script_dir/enable-ssh.py" "$serial_port" "$vm_dir/key.pub"
  for _ in $(seq 1 30); do
    vm_ssh true && return 0
    sleep 2
  done
  echo "::error::the NetBSD VM did not accept SSH connections" >&2
  return 1
}

install_rust() {
  download "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-netbsd/rustup-init" "$vm_dir/rustup-init"
  echo "$RUSTUP_SHA256  $vm_dir/rustup-init" | sha256sum --check --quiet
  vm_ssh 'cat > /tmp/rustup-init && chmod +x /tmp/rustup-init' < "$vm_dir/rustup-init"

  local channel
  channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$repo_root/rust-toolchain.toml")
  vm_ssh "mkdir /build && cd /build && tar xf /dev/rwd1d && /tmp/rustup-init -y --profile minimal --default-toolchain $channel"
}

build() {
  local target='' profile=release
  local arg previous=''
  for arg in "$@"; do
    case "$previous" in
      --target) target=$arg ;;
      --profile) profile=$arg ;;
    esac
    case "$arg" in
      --target=*) target=${arg#--target=} ;;
      --profile=*) profile=${arg#--profile=} ;;
    esac
    previous=$arg
  done
  if [ -z "$target" ]; then
    echo "::error::vm.sh build needs --target, so the output lands where the release steps look for it" >&2
    return 1
  fi

  vm_ssh ". \$HOME/.cargo/env && cd /build && cargo build --offline $(printf '%q ' "$@")"
  local output_dir="target/$target/$profile"
  mkdir -p "$repo_root/$output_dir"
  vm_ssh "cd /build/$output_dir && tar cf - \$(find . -maxdepth 1 -type f)" | tar xf - -C "$repo_root/$output_dir"
}

command=${1:-}
shift || true
case "$command" in
  start)
    mkdir -p "$vm_dir"
    install_netbsd
    pack_source
    boot
    install_rust
    ;;
  build) build "$@" ;;
  run) vm_ssh "$@" ;;
  *)
    echo "usage: vm.sh start | build <cargo args> | run <command>" >&2
    exit 2
    ;;
esac
