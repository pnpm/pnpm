"""Interpreter capabilities and wheel layout; networking and resolution belong to pnpm."""

import base64
import configparser
import csv
import email.parser
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import sys
import sysconfig
import venv


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


def probe():
    markers, tags = packaging_modules()
    return {
        "executable": sys.executable,
        "environment": markers.default_environment(),
        "tags": [str(tag) for tag in tags.sys_tags()],
    }


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


def install(request):
    root = Path(request["root"])
    venv.EnvBuilder(with_pip=False, symlinks=os.name != "nt").create(root)
    variables = {"base": str(root), "platbase": str(root)}
    scheme_name = "venv" if "venv" in sysconfig.get_scheme_names() else ("nt" if os.name == "nt" else "posix_prefix")
    scheme = sysconfig.get_paths(scheme=scheme_name, vars=variables)
    scheme["headers"] = str(root / "include" / "site" / ("python" + sysconfig.get_python_version()))
    scripts = Path(scheme["scripts"])
    interpreter = scripts / ("python.exe" if os.name == "nt" else "python")
    occupied = set()
    for package in request["packages"]:
        files, metadata = package["files"], package["metadata"]
        dist_info = metadata["dist_info"]
        site = Path(scheme["purelib" if metadata["purelib"] else "platlib"])
        record_path = site / dist_info / "RECORD"
        records = []

        def write(destination, contents, executable=False):
            destination = Path(destination)
            if not destination.is_relative_to(root):
                raise ValueError("wheel destination escapes environment")
            key = os.path.normcase(str(destination))
            if key in occupied or destination.exists():
                raise ValueError("Python package file collision: " + str(destination.relative_to(root)))
            occupied.add(key)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(contents)
            if executable:
                destination.chmod(0o755)
            digest = base64.urlsafe_b64encode(hashlib.sha256(contents).digest()).rstrip(b"=").decode()
            records.append([os.path.relpath(destination, site).replace(os.sep, "/"), "sha256=" + digest, str(len(contents))])

        for name, source in files.items():
            if name in (dist_info + "/RECORD", dist_info + "/RECORD.jws", dist_info + "/RECORD.p7s", dist_info + "/INSTALLER"):
                continue
            parts = PurePosixPath(name).parts
            executable = bool(Path(source).stat().st_mode & 0o111)
            if parts[0].endswith(".data"):
                if parts[0] != dist_info.removesuffix(".dist-info") + ".data" or len(parts) < 3 or parts[1] not in scheme:
                    raise ValueError("invalid wheel data path: " + name)
                destination = Path(scheme[parts[1]]).joinpath(*parts[2:])
                executable = executable or parts[1] == "scripts"
            else:
                destination = site.joinpath(*parts)
            contents = Path(source).read_bytes()
            if executable and (contents.startswith(b"#!python\n") or contents.startswith(b"#!pythonw\n")):
                contents = ("#!" + str(interpreter) + "\n").encode() + contents.split(b"\n", 1)[1]
            write(destination, contents, executable)

        entry_points = files.get(dist_info + "/entry_points.txt")
        if entry_points:
            entries = configparser.ConfigParser(interpolation=None)
            entries.optionxform = str
            entries.read(entry_points, encoding="utf-8")
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
                        write(scripts / (name + "-script.py"), body.encode())
                        launcher = '@"' + str(interpreter) + '" "%~dp0' + name + '-script.py" %*\r\n'
                        write(scripts / (name + ".cmd"), launcher.encode())
                    else:
                        # A shell trampoline supports interpreter paths containing spaces and long paths.
                        import shlex
                        prefix = "#!/bin/sh\n'''exec' " + shlex.quote(str(interpreter)) + ' "$0" "$@"\n\x27 \x27\x27\x27\n'
                        write(scripts / name, (prefix + body).encode(), True)
        write(site / dist_info / "INSTALLER", b"pnpm\n")
        records.append([dist_info + "/RECORD", "", ""])
        record_path.parent.mkdir(parents=True, exist_ok=True)
        with record_path.open("w", encoding="utf-8", newline="") as record:
            csv.writer(record).writerows(sorted(records))
    return {"root": str(root)}


request = json.load(sys.stdin)
result = {"probe": probe, "inspect": lambda: inspect_wheel(request), "install": lambda: install(request)}[sys.argv[1]]()
json.dump(result, sys.stdout)
