#!/usr/bin/env python3
"""Install prebuilt Creusot tools from a release of this repository.

Each tool is a separate asset, so a task that only translates a crate to Coma can
take the tools it needs and leave the prover framework undownloaded. The release
describes itself in `release.json`: this script reads which assets exist, where
each one unpacks and what each one's checksum is, and hardcodes none of it.

    install.py --list
    install.py cargo-creusot creusot-rustc
    install.py --all
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeVar

DEFAULT_REPOSITORY = "KSXGitHub/creusot-prebuilt"
RELEASE_METADATA = "release.json"

CREUSOT_DATA_HOME = "creusot-data-home"
WHY3_PREFIX = "why3-prefix"

LATEST = "latest"

PINNED_RELEASE: str | None = "0.15.0-0"
"""The release to install when `--release` is not given, or `None` for the newest.

An `install.py` downloaded from a release carries that release's tag here, so
it installs the release it came from rather than whatever is newest. Passing
`--release latest` asks for the newest either way.
"""


def say(message: str = "") -> None:
    """Tell the person something.

    Narration goes to stderr so that a caller redirecting stdout catches only
    what it asked for. `--list` is the exception: its listing is the answer to
    the question, so it goes to stdout.
    """
    sys.stderr.write(f"{message}\n")
    sys.stderr.flush()


class InstallError(Exception):
    """Something the caller has to fix: a missing tool, a bad checksum, a read-only prefix."""


@dataclass(frozen=True)
class AssetDescription:
    name: str
    file: str
    prefix: str
    summary: str
    sha256: str
    size: int


@dataclass(frozen=True)
class ReleaseDescription:
    tag: str
    target: str
    rust_channel: str
    why3_prefix: str
    assets: tuple[AssetDescription, ...]

    def asset(self, name: str) -> AssetDescription:
        for candidate in self.assets:
            if candidate.name == name:
                return candidate
        known = ", ".join(candidate.name for candidate in self.assets)
        raise InstallError(f"{name!r} is not in this release; it has: {known}")


FieldType = TypeVar("FieldType")


def _field(source: dict[str, Any], key: str, kind: type[FieldType]) -> FieldType:
    if key not in source:
        raise InstallError(f"{RELEASE_METADATA} has no {key!r}")
    value = source[key]
    if not isinstance(value, kind):
        raise InstallError(f"{RELEASE_METADATA} field {key!r} should be {kind.__name__}")
    return value


def parse_release(text: str) -> ReleaseDescription:
    document = json.loads(text)
    if not isinstance(document, dict):
        raise InstallError(f"{RELEASE_METADATA} is not an object")
    raw_assets = _field(document, "assets", list)
    assets = tuple(
        AssetDescription(
            name=_field(entry, "name", str),
            file=_field(entry, "file", str),
            prefix=_field(entry, "prefix", str),
            summary=_field(entry, "summary", str),
            sha256=_field(entry, "sha256", str),
            size=_field(entry, "size", int),
        )
        for entry in raw_assets
    )
    return ReleaseDescription(
        tag=_field(document, "tag", str),
        target=_field(document, "target", str),
        rust_channel=_field(document, "rust_channel", str),
        why3_prefix=_field(document, "why3_prefix", str),
        assets=assets,
    )


def data_home(override: Path | None) -> Path:
    """Where `cargo-creusot` looks, following the same rules it does."""
    if override is not None:
        return override
    from_env = os.environ.get("CREUSOT_DATA_HOME")
    if from_env:
        return Path(from_env)
    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / "creusot"
    return Path.home() / ".local" / "share" / "creusot"


def fetch(url: str) -> bytes:
    try:
        with urllib.request.urlopen(url) as response:
            body: bytes = response.read()
            return body
    except urllib.error.HTTPError as error:
        raise InstallError(f"{url} answered {error.code} {error.reason}") from error
    except urllib.error.URLError as error:
        raise InstallError(f"{url} could not be reached: {error.reason}") from error


def latest_tag(repository: str) -> str:
    document = json.loads(fetch(f"https://api.github.com/repos/{repository}/releases/latest"))
    if not isinstance(document, dict):
        raise InstallError("the GitHub release API returned something unexpected")
    return _field(document, "tag_name", str)


class Source:
    """Where the release's files are read from."""

    def __init__(self, repository: str, tag: str | None, directory: Path | None) -> None:
        self.directory = directory
        self.repository = repository
        if directory is not None:
            self.tag = tag or ""
        else:
            self.tag = tag or latest_tag(repository)

    def read_bytes(self, name: str) -> bytes:
        """The content of the named release file."""
        if self.directory is None:
            return fetch(f"https://github.com/{self.repository}/releases/download/{self.tag}/{name}")
        local = self.directory / name
        if not local.is_file():
            raise InstallError(f"{local} does not exist")
        return local.read_bytes()

    def copy_to(self, name: str, destination: Path) -> Path:
        """Put the named release file at `destination` and return it."""
        destination.write_bytes(self.read_bytes(name))
        return destination


def human_size(size: int) -> str:
    """A size a reader can compare at a glance, in the unit that suits it."""
    return f"{size / 1e6:.1f} MB" if size >= 1_000_000 else f"{size / 1e3:.0f} kB"


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def safe_members(archive: tarfile.TarFile, prefix: Path) -> list[tarfile.TarInfo]:
    """Every member of the archive, once each one is known to stay under `prefix`.

    An archive from a release of this repository holds only regular files,
    directories and relative symlinks, so anything else is a reason to stop
    rather than something to skip quietly.
    """
    members: list[tarfile.TarInfo] = []
    for member in archive.getmembers():
        if not (member.isfile() or member.isdir() or member.issym()):
            raise InstallError(f"{member.name} is neither a file, a directory nor a symlink")
        resolved = os.path.normpath(prefix / member.name)
        if not str(resolved).startswith(str(prefix) + os.sep):
            raise InstallError(f"{member.name} would be written outside {prefix}")
        if member.issym() and os.path.isabs(member.linkname):
            raise InstallError(f"{member.name} points at the absolute path {member.linkname}")
        members.append(member)
    return members


def install_asset(asset: AssetDescription, source: Source, prefix: Path, dry_run: bool) -> list[str]:
    """Unpack one asset at its prefix and report the paths it put there."""
    say(f"  {asset.name}: {human_size(asset.size)} into {prefix}")
    if dry_run:
        return []
    with tempfile.TemporaryDirectory() as scratch:
        archive_path = source.copy_to(asset.file, Path(scratch) / asset.file)
        actual = sha256_of(archive_path)
        if actual != asset.sha256:
            raise InstallError(f"{asset.file} hashes to {actual}, not the {asset.sha256} the release records")
        prefix.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive_path, "r:gz") as archive:
            # `safe_members` reports a bad member by name; `filter="data"`
            # is the interpreter's own guard behind it.
            members = safe_members(archive, prefix.resolve())
            archive.extractall(prefix, members=members, filter="data")
            return [member.name for member in members if member.isfile()]


def link_into_data_bin(data: Path, target: Path) -> None:
    """Put a tool `cargo-creusot` spawns where it looks: the data directory's `bin`."""
    link = data / "bin" / target.name
    link.parent.mkdir(parents=True, exist_ok=True)
    if link.is_symlink() or link.exists():
        link.unlink()
    link.symlink_to(target)
    say(f"  linked {link} -> {target}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("tools", nargs="*", metavar="TOOL", help="the tools to install")
    parser.add_argument("--all", action="store_true", help="install every tool in the release")
    parser.add_argument("--list", action="store_true", help="print the tools in the release and stop")
    parser.add_argument(
        "--release",
        default=PINNED_RELEASE,
        metavar="TAG",
        help=f"which release to install, or {LATEST} for the newest (default: {PINNED_RELEASE or LATEST})",
    )
    parser.add_argument("--repository", default=DEFAULT_REPOSITORY, help="owner/name to download from")
    parser.add_argument(
        "--from-dir",
        type=Path,
        default=None,
        metavar="DIR",
        help="read the release from a directory instead of downloading it",
    )
    parser.add_argument(
        "--data-home", type=Path, default=None, metavar="DIR", help="override where the Creusot data directory is"
    )
    parser.add_argument("--dry-run", action="store_true", help="say what would be installed and stop")
    arguments = parser.parse_args(argv)

    wanted_tag = None if arguments.release == LATEST else arguments.release
    source = Source(arguments.repository, wanted_tag, arguments.from_dir)
    description = parse_release(source.read_bytes(RELEASE_METADATA).decode("utf-8"))

    if arguments.list:
        sys.stdout.write(f"{description.tag} for {description.target}, built against {description.rust_channel}\n")
        for asset in description.assets:
            sys.stdout.write(f"  {asset.name:<16} {human_size(asset.size):>9}  {asset.summary}\n")
        return 0

    if arguments.all:
        wanted = [asset.name for asset in description.assets]
    elif arguments.tools:
        wanted = list(dict.fromkeys(arguments.tools))
    else:
        parser.error("name at least one tool, or pass --all or --list")

    data = data_home(arguments.data_home)
    why3_prefix = Path(description.why3_prefix)
    prefixes = {CREUSOT_DATA_HOME: data, WHY3_PREFIX: why3_prefix}

    say(f"installing {description.tag} for {description.target}")
    assets = [description.asset(name) for name in wanted]
    installed_at_why3_prefix: list[str] = []
    for asset in assets:
        prefix = prefixes.get(asset.prefix)
        if prefix is None:
            raise InstallError(f"{asset.name} unpacks at the unknown prefix {asset.prefix!r}")
        written = install_asset(asset, source, prefix, arguments.dry_run)
        if asset.prefix == WHY3_PREFIX:
            installed_at_why3_prefix += written

    if arguments.dry_run:
        return 0

    # `cargo-creusot` spawns a tool by looking in the data directory's `bin`, and an asset that
    # unpacks somewhere else has to be reachable from there. Only the programs
    # this run unpacked are linked, so an unrelated binary that already sits
    # beside them is left alone.
    for member in sorted(installed_at_why3_prefix):
        if Path(member).parent == Path("bin"):
            link_into_data_bin(data, why3_prefix / member)

    say()
    say(f"installed into {data}" + (f" and {why3_prefix}" if any(a.prefix == WHY3_PREFIX for a in assets) else ""))
    say(f"put {data / 'bin'} on PATH, then `cargo creusot --help`")
    if any(asset.name == "creusot-install" for asset in assets):
        say(f"run `CREUSOT_DATA_HOME={data} creusot-install provers` to add the SMT solvers")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except InstallError as error:
        say(f"install: {error}")
        sys.exit(1)
