"""Interpreter capabilities and wheel layout; networking and resolution belong to pnpm."""

import base64
import configparser
import csv
import email.parser
import hashlib
import importlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shlex
import shutil
import sys
import sysconfig
import venv
import zipfile


def packaging_modules():
    try:
        from packaging import markers, tags
    except ImportError:
        try:
            from pip._vendor.packaging import markers, tags
        except ImportError as error:
            raise RuntimeError(
                "pnpm Python integration requires packaging (or pip) in the configured interpreter"
            ) from error
    return markers, tags


def probe(request):
    markers, tags = packaging_modules()
    environment = markers.default_environment()
    running = [str(tag) for tag in tags.sys_tags()]
    return {
        "executable": sys.executable,
        "environment": environment,
        "tags": running,
        "targets": [declared_target(environment, running, entry) for entry in request.get("targets", [])],
    }


# The oldest glibc, musl and macOS a declared environment is resolved for
# when its name does not say. A wheel built for a newer one is not offered
# to that environment.
DEFAULT_LIBC = {"gnu": "manylinux_2_17", "musl": "musllinux_1_2"}
MACOS_VERSION = (14, 0)
PLATFORM_ALIASES = {
    "linux": "x86_64-unknown-linux-gnu",
    "macos": "aarch64-apple-darwin",
    "windows": "x86_64-pc-windows-msvc",
}
# The glibc versions that also have a pre-PEP 600 tag, and the
# architectures each of those tags was ever defined for.
MANYLINUX_LEGACY = {5: "manylinux1", 12: "manylinux2010", 17: "manylinux2014"}
LEGACY_ARCHITECTURES = {
    "manylinux1": {"x86_64", "i686"},
    "manylinux2010": {"x86_64", "i686"},
    "manylinux2014": {"x86_64", "i686", "aarch64", "armv7l", "ppc64", "ppc64le", "s390x"},
}
# The libc series wheels are tagged for, with the oldest and newest release
# of each a platform can name. A number outside them is a typo rather than a
# baseline, and counting down from it would take unbounded time and memory.
LIBC_BASELINES = {"manylinux": ("2", 5, 99), "musllinux": ("1", 0, 99)}
# Python's wheel tags spell some architectures differently from the Rust
# target triple that names the same machine.
WHEEL_ARCHITECTURES = {"powerpc64": "ppc64", "powerpc64le": "ppc64le", "riscv64gc": "riscv64"}
DARWIN_MACHINES = {"x86_64": "x86_64", "aarch64": "arm64"}
WINDOWS_MACHINES = {"x86_64": ("AMD64", "win_amd64"), "aarch64": ("ARM64", "win_arm64"), "i686": ("x86", "win32")}


def describe_platform(name):
    """The marker variables a declared platform fixes, and the wheel platform tags it accepts."""
    _, tags = packaging_modules()
    architecture, separator, system = PLATFORM_ALIASES.get(name, name).partition("-")
    architecture = WHEEL_ARCHITECTURES.get(architecture, architecture)
    system = DEFAULT_LIBC.get(system.removeprefix("unknown-linux-"), system)
    if separator and (system.startswith("manylinux_") or system.startswith("musllinux_")):
        return linux_platform(architecture, system)
    if system == "apple-darwin" and architecture in DARWIN_MACHINES:
        machine = DARWIN_MACHINES[architecture]
        variables = {"os_name": "posix", "sys_platform": "darwin", "platform_system": "Darwin", "platform_machine": machine}
        return variables, [str(platform) for platform in tags.mac_platforms(MACOS_VERSION, machine)]
    if system == "pc-windows-msvc" and architecture in WINDOWS_MACHINES:
        machine, platform_tag = WINDOWS_MACHINES[architecture]
        variables = {"os_name": "nt", "sys_platform": "win32", "platform_system": "Windows", "platform_machine": machine}
        return variables, [platform_tag]
    raise ValueError("pnpm does not know the Python platform " + name)


def linux_platform(architecture, libc):
    """A glibc or musl Linux platform, with every libc release its wheels may be built against."""
    kind, major, minor = libc.rsplit("_", 2)
    series = LIBC_BASELINES[kind]
    if major != series[0] or not minor.isdigit() or not series[1] <= int(minor) <= series[2]:
        raise ValueError("pnpm does not know the Python libc baseline " + libc)
    platforms = []
    oldest = series[1]
    for release in range(int(minor), oldest - 1, -1):
        platforms.append("%s_%s_%d_%s" % (kind, major, release, architecture))
        legacy = MANYLINUX_LEGACY.get(release) if kind == "manylinux" and major == "2" else None
        if legacy and architecture in LEGACY_ARCHITECTURES[legacy]:
            platforms.append(legacy + "_" + architecture)
    platforms.append("linux_" + architecture)
    variables = {"os_name": "posix", "sys_platform": "linux", "platform_system": "Linux", "platform_machine": architecture}
    return variables, platforms


def declared_target(running_environment, running_tags, entry):
    """What one environment the project declares resolves as: its markers and the wheels it takes.

    A declared environment is a CPython interpreter on a named platform.
    A platform pnpm never runs on reports no kernel release or build.
    """
    _, tags = packaging_modules()
    platform_name, version = entry.get("platform"), entry.get("python")
    if platform_name is None and version is None:
        return {"environment": running_environment, "tags": running_tags}
    environment = dict(running_environment)
    if platform_name is None:
        # "any" is not a platform: compatible_tags adds the tags carrying it.
        running_platforms = (tag.rsplit("-", 1)[-1] for tag in running_tags)
        platforms = list(dict.fromkeys(tag for tag in running_platforms if tag != "any"))
    else:
        variables, platforms = describe_platform(platform_name)
        environment.update(variables, platform_release="", platform_version="")
    if version is None:
        release = tuple(int(part) for part in running_environment["python_version"].split("."))[:2]
    else:
        parts = version.split(".")
        if len(parts) not in (2, 3) or not all(part.isdigit() for part in parts):
            raise ValueError("a Python version pnpm locks for reads 3.12 or 3.12.7, not " + version)
        release = (int(parts[0]), int(parts[1]))
        environment.update(
            python_version="%d.%d" % release,
            python_full_version=version if len(parts) == 3 else version + ".0",
            implementation_version=version if len(parts) == 3 else version + ".0",
        )
    environment.update(implementation_name="cpython", platform_python_implementation="CPython")
    interpreter = "cp%d%d" % release
    # The stable ABI of a release: CPython before 3.8 carries the pymalloc
    # flag in its tag, and a free-threaded build is one pnpm cannot be asked for.
    abi = interpreter + "m" if release < (3, 8) else interpreter
    declared = list(tags.cpython_tags(python_version=release, abis=[abi], platforms=platforms))
    declared += list(tags.compatible_tags(python_version=release, interpreter=interpreter, platforms=platforms))
    return {"environment": environment, "tags": [str(tag) for tag in declared]}


def read_headers(files, name):
    return email.parser.Parser().parsestr(Path(files[name]).read_text(encoding="utf-8"))


def inspect_wheel(request):
    files = request["files"]
    roots = {name.split("/", 1)[0] for name in files if name.split("/", 1)[0].endswith(".dist-info")}
    if len(roots) != 1:
        raise ValueError("wheel must contain exactly one dist-info directory")
    dist_info = roots.pop()
    for name in files:
        parts = PurePosixPath(name).parts
        if not parts or any(part in (".", "..") for part in parts) or name.startswith("/") or "\\" in name:
            raise ValueError("unsafe wheel path: " + name)
    wheel = read_headers(files, dist_info + "/WHEEL")
    if wheel["Wheel-Version"] != "1.0":
        raise ValueError("unsupported Wheel-Version: " + str(wheel["Wheel-Version"]))
    if wheel["Root-Is-Purelib"] not in ("true", "false"):
        raise ValueError("invalid Root-Is-Purelib")
    _, tags = packaging_modules()
    filename_tags = tags.parse_tag("-".join(request["filename"].removesuffix(".whl").rsplit("-", 3)[1:]))
    declared_tags = set()
    for tag in wheel.get_all("Tag", []):
        declared_tags.update(tags.parse_tag(tag))
    if declared_tags != filename_tags:
        raise ValueError("wheel Tag fields do not match filename: " + request["filename"])
    metadata = read_headers(files, dist_info + "/METADATA")
    for field in ("Name", "Version", "Metadata-Version"):
        if len(metadata.get_all(field, [])) != 1:
            raise ValueError("wheel requires exactly one " + field)
    if not metadata["Metadata-Version"].startswith("2."):
        raise ValueError("unsupported Metadata-Version")
    record_name = dist_info + "/RECORD"
    recorded = set()
    for row in csv.reader(io.StringIO(Path(files[record_name]).read_text(encoding="utf-8"))):
        if len(row) != 3:
            raise ValueError("invalid wheel RECORD row")
        name, digest, size = row
        if name in recorded or name not in files:
            raise ValueError("duplicate or missing RECORD file: " + name)
        recorded.add(name)
        if name == record_name:
            if digest or size:
                raise ValueError("RECORD must not hash itself")
            continue
        algorithm, separator, expected = digest.partition("=")
        if not separator or algorithm not in ("sha256", "sha384", "sha512"):
            raise ValueError("unsupported wheel RECORD hash: " + name)
        contents = Path(files[name]).read_bytes()
        actual = base64.urlsafe_b64encode(hashlib.new(algorithm, contents).digest()).rstrip(b"=").decode()
        if actual != expected or int(size) != len(contents):
            raise ValueError("wheel RECORD verification failed: " + name)
    unsigned = set(files) - recorded
    if unsigned - {record_name + ".jws", record_name + ".p7s"}:
        raise ValueError("wheel RECORD does not cover every file")
    return {
        "name": metadata["Name"],
        "version": metadata["Version"],
        "requires_dist": metadata.get_all("Requires-Dist", []),
        "requires_python": metadata.get("Requires-Python"),
        "provides_extra": metadata.get_all("Provides-Extra", []),
        "dist_info": dist_info,
        "purelib": wheel["Root-Is-Purelib"] == "true",
    }


class Environment:
    """One environment's files: what is written where, and what each installed distribution records."""

    def __init__(self, root, scheme):
        self.root = root
        self.scheme = scheme
        self.scripts = Path(scheme["scripts"])
        self.interpreter = self.scripts / ("python.exe" if os.name == "nt" else "python")
        self.occupied = set()

    def start(self, purelib):
        self.site = Path(self.scheme["purelib" if purelib else "platlib"])
        self.records = []

    def write(self, destination, contents, executable=False):
        destination = Path(destination)
        if not destination.is_relative_to(self.root):
            raise ValueError("wheel destination escapes environment")
        key = os.path.normcase(str(destination))
        if key in self.occupied or destination.exists():
            raise ValueError("Python package file collision: " + str(destination.relative_to(self.root)))
        self.occupied.add(key)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)
        if executable:
            destination.chmod(0o755)
        digest = base64.urlsafe_b64encode(hashlib.sha256(contents).digest()).rstrip(b"=").decode()
        self.records.append([os.path.relpath(destination, self.site).replace(os.sep, "/"), "sha256=" + digest, str(len(contents))])

    def write_entry_points(self, entries):
        for group in ("console_scripts", "gui_scripts"):
            for name, entry in entries.items(group) if entries.has_section(group) else []:
                if not name or not all(character.isascii() and (character.isalnum() or character in "-_.") for character in name):
                    raise ValueError("unsafe entry point name: " + name)
                module, separator, function = entry.split("[", 1)[0].strip().partition(":")
                if not separator or not all(part.isidentifier() for part in module.split(".")) or not all(part.isidentifier() for part in function.strip().split(".")):
                    raise ValueError("invalid Python entry point: " + entry)
                function = function.strip()
                body = "import sys\nfrom " + module + " import " + function.split(".")[0] + "\nif __name__ == '__main__':\n    sys.exit(" + function + "())\n"
                if os.name == "nt":
                    self.write(self.scripts / (name + "-script.py"), body.encode())
                    launcher = '@"' + str(self.interpreter) + '" "%~dp0' + name + '-script.py" %*\r\n'
                    self.write(self.scripts / (name + ".cmd"), launcher.encode())
                else:
                    # A shell trampoline supports interpreter paths containing spaces and long paths.
                    prefix = "#!/bin/sh\n'''exec' " + shlex.quote(str(self.interpreter)) + ' "$0" "$@"\n\x27 \x27\x27\x27\n'
                    self.write(self.scripts / name, (prefix + body).encode(), True)

    def finish(self, dist_info):
        self.write(self.site / dist_info / "INSTALLER", b"pnpm\n")
        self.records.append([dist_info + "/RECORD", "", ""])
        record_path = self.site / dist_info / "RECORD"
        record_path.parent.mkdir(parents=True, exist_ok=True)
        with record_path.open("w", encoding="utf-8", newline="") as record:
            csv.writer(record).writerows(sorted(self.records))


def install_wheel(environment, package):
    files, metadata = package["files"], package["metadata"]
    dist_info = metadata["dist_info"]
    environment.start(metadata["purelib"])
    for name, source in files.items():
        if name in (dist_info + "/RECORD", dist_info + "/RECORD.jws", dist_info + "/RECORD.p7s", dist_info + "/INSTALLER"):
            continue
        parts = PurePosixPath(name).parts
        executable = bool(Path(source).stat().st_mode & 0o111)
        if parts[0].endswith(".data"):
            if parts[0] != dist_info.removesuffix(".dist-info") + ".data" or len(parts) < 3 or parts[1] not in environment.scheme:
                raise ValueError("invalid wheel data path: " + name)
            destination = Path(environment.scheme[parts[1]]).joinpath(*parts[2:])
            executable = executable or parts[1] == "scripts"
        else:
            destination = environment.site.joinpath(*parts)
        contents = Path(source).read_bytes()
        if executable and contents.startswith(b"#!python"):
            first_line, newline, body = contents.partition(b"\n")
            if first_line.removesuffix(b"\r") in (b"#!python", b"#!pythonw"):
                contents = ("#!" + str(environment.interpreter)).encode() + newline + body
        environment.write(destination, contents, executable)

    entry_points = files.get(dist_info + "/entry_points.txt")
    if entry_points:
        entries = configparser.ConfigParser(interpolation=None)
        entries.optionxform = str
        entries.read(entry_points, encoding="utf-8")
        environment.write_entry_points(entries)
    # PEP 610 has the installer record where a distribution came from,
    # which a wheel built from a directory cannot carry itself.
    if package.get("direct_url"):
        origin = package["direct_url"]
        environment.write(
            environment.site / dist_info / "direct_url.json",
            json.dumps({"url": origin["url"], "dir_info": {"editable": origin["editable"]}}).encode("utf-8"),
        )
    environment.finish(dist_info)


def unpack(archive, target):
    """Extract a wheel, reporting each file the way `inspect` and `install` read one."""
    files = {}
    with zipfile.ZipFile(archive) as contents:
        for entry in contents.infolist():
            if entry.is_dir():
                continue
            parts = PurePosixPath(entry.filename).parts
            if not parts or entry.filename.startswith("/") or "\\" in entry.filename or any(part in (".", "..") for part in parts):
                raise ValueError("unsafe wheel path: " + entry.filename)
            destination = target.joinpath(*parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            with contents.open(entry) as source, destination.open("wb") as sink:
                shutil.copyfileobj(source, sink)
            if entry.external_attr >> 16 & 0o111:
                destination.chmod(0o755)
            files[entry.filename] = str(destination)
    return files


def load_backend(request):
    """Import the project's PEP 517 backend, with the project as the working directory.

    PEP 660's editable hooks are optional, so a backend that does not
    implement them builds an ordinary wheel instead.
    """
    root = Path(request["root"]).resolve()
    for entry in reversed(request["backend_path"]):
        # PEP 517 gives backend-path the project as its root and forbids
        # it from reaching outside, so a manifest cannot put an arbitrary
        # directory of the machine on the import path.
        located = root.joinpath(entry).resolve()
        if located != root and root not in located.parents:
            raise ValueError("backend-path escapes the project: " + entry)
        sys.path.insert(0, str(located))
    # The backend reads the project, so it is imported from within it.
    os.chdir(root)
    module, _, attribute = request["backend"].partition(":")
    backend = importlib.import_module(module)
    for part in filter(None, attribute.split(".")):
        backend = getattr(backend, part)
    editable = request["editable"] and hasattr(backend, "build_editable")
    return backend, editable


def build_requires(request):
    """What the backend needs installed beyond `build-system.requires` to build this project."""
    backend, editable = load_backend(request)
    name = "get_requires_for_build_editable" if editable else "get_requires_for_build_wheel"
    hook = getattr(backend, name, None)
    return list(hook()) if hook is not None else []


def build(request):
    backend, editable = load_backend(request)
    output = Path(request["output"]).resolve()
    hook = backend.build_editable if editable else backend.build_wheel
    filename = hook(str(output))
    # Reading the wheel belongs to the interpreter it is installed for, not
    # to the environment the backend needed, which holds only the backend.
    return {"files": unpack(output / filename, output / "unpacked"), "filename": filename}


def install(request):
    root = Path(request["root"])
    venv.EnvBuilder(with_pip=False, symlinks=os.name != "nt").create(root)
    variables = {"base": str(root), "platbase": str(root)}
    scheme_name = "venv" if "venv" in sysconfig.get_scheme_names() else ("nt" if os.name == "nt" else "posix_prefix")
    scheme = sysconfig.get_paths(scheme=scheme_name, vars=variables)
    scheme["headers"] = str(root / "include" / "site" / ("python" + sysconfig.get_python_version()))
    environment = Environment(root, scheme)
    for package in request["packages"]:
        install_wheel(environment, package)
    return {"root": str(root)}


request = json.load(sys.stdin)
# A build backend may write to stdout, and some run a child process that
# writes to the descriptor directly. The answer to pnpm is what stdout
# carries, so the operation gets stderr and the answer goes to a copy.
answer = os.fdopen(os.dup(1), "w")
os.dup2(2, 1)
sys.stdout = sys.stderr
result = {"probe": probe, "inspect": inspect_wheel, "install": install, "build": build, "build_requires": build_requires}[sys.argv[1]](request)
json.dump(result, answer)
answer.flush()
