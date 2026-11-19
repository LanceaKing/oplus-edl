#!/usr/bin/env python3
"""Stage and verify the three desktop release layouts from locked, local inputs."""

import argparse
import configparser
import hashlib
import json
import plistlib
import re
import shutil
import stat
import struct
import subprocess
import tarfile
import zipfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
FEATURES = {"cloud", "serial", "usb"}
PRODUCTS = ("oplus-edl-cli", "oplus-edl-desktop")
TARGETS = {
    "aarch64-apple-darwin": ("macos-arm64", "zip", "oplus-edl", "Oplus EDL.app/Contents/MacOS/oplus-edl-desktop"),
    "x86_64-pc-windows-msvc": ("windows-x86_64", "zip", "oplus-edl.exe", "oplus-edl-desktop.exe"),
    "x86_64-unknown-linux-gnu": ("linux-x86_64", "tar.gz", "bin/oplus-edl", "bin/oplus-edl-desktop"),
}
MAC_APP = "Oplus EDL.app"
PLIST = f"{MAC_APP}/Contents/Info.plist"
CODE_RESOURCES = f"{MAC_APP}/Contents/_CodeSignature/CodeResources"
DESKTOP_ENTRY = "share/applications/oplus-edl-desktop.desktop"
BUNDLE_ID = "io.github.lanceaking.oplus-edl.desktop"
GUIDES = ("docs/distribution.md", "docs/desktop.md", "docs/cli.md")
LICENSES = {
    "licenses/qdlrs-BSD-3-Clause.txt": "licenses/qdlrs-BSD-3-Clause.txt",
    "licenses/egui-MIT.txt": "licenses/egui-MIT.txt",
    "licenses/rfd-MIT.txt": "licenses/rfd-MIT.txt",
    "licenses/NotoSansCJK-OFL.txt": "crates/oplus-edl-desktop/assets/fonts/LICENSE",
}
MANIFEST = "MANIFEST.sha256"
BUILD = "BUILD.json"
INDEX = "licenses/dependencies.json"
BORINGSSL = "licenses/boringssl-LICENSE.txt"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def cargo(*args):
    return subprocess.check_output(
        ["cargo", *args, "--locked", "--offline", "--color", "never"], cwd=ROOT
    )


def production_graph(target, features):
    selection = ("--no-default-features", "--features", features)
    tree = cargo(
        "tree", "-p", PRODUCTS[0], "-p", PRODUCTS[1], "--target", target,
        "--edges", "normal,build", "--prefix", "none", "--format", "{p}|{f}", *selection
    ).decode("utf-8")
    graph = {}
    for line in tree.splitlines():
        if not line.strip():
            continue
        package, separator, enabled = line.removesuffix(" (*)").rpartition("|")
        match = re.match(r"([\w-]+) v([^\s]+)", package)
        require(separator and match is not None, f"unexpected Cargo tree row: {line}")
        graph.setdefault(match.groups(), set()).update(filter(None, enabled.split(",")))

    metadata = json.loads(cargo(
        "metadata", "--format-version", "1", "--filter-platform", target, *selection
    ))
    packages = {}
    for package in metadata["packages"]:
        key = package["name"], package["version"]
        if key in graph:
            require(key not in packages, f"ambiguous Cargo source for {key}")
            packages[key] = package
    require(packages.keys() == graph.keys(), "Cargo tree and metadata disagree")

    version = None
    for name in (*PRODUCTS, "oplus-edl-core"):
        entries = [(key, enabled) for key, enabled in graph.items() if key[0] == name]
        require(len(entries) == 1, f"missing or ambiguous production package {name}")
        key, enabled = entries[0]
        expected = FEATURES.copy()
        if name == "oplus-edl-core" and "default" in enabled:
            require(packages[key]["features"].get("default") == [], "core defaults must be empty")
            expected.add("default")
        require(enabled == expected, f"{name} has unexpected features: {sorted(enabled)}")
        require(version is None or version == key[1], "workspace versions disagree")
        version = key[1]

    require(re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.+-]+)?", version), "unsafe release version")
    return version, packages


def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def write_json(destination, value):
    destination.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def dependency_notices(destination, packages):
    rows = []
    notice_names = ("LICENSE", "LICENCE", "COPYRIGHT", "NOTICE", "COPYING", "UNLICENSE")
    upstream = ROOT / "licenses/upstream"
    catalog = json.loads((upstream / "index.json").read_text(encoding="utf-8"))
    supplemental = {crate: (filename, notice["sources"])
                    for filename, notice in catalog.items() for crate in notice["packages"]}
    for (name, version), package in sorted(packages.items()):
        if name in (*PRODUCTS, "oplus-edl-core"):
            continue
        directory = Path(package["manifest_path"]).parent
        paths = [path for path in directory.iterdir()
                 if path.is_file() and path.name.upper().startswith(notice_names)]
        if package["license_file"]:
            paths.append(directory / package["license_file"])
        bundled = []
        for index, path in enumerate(sorted(set(paths)), start=1):
            rel = f"licenses/cargo/{name}-{version}/{index}-{path.name}"
            copy(path, destination / rel)
            bundled.append(rel)
        if name == "btls-sys":
            copy(directory / "deps/boringssl/LICENSE", destination / BORINGSSL)
            bundled.append(BORINGSSL)
        if name == "qdl":
            bundled.append("licenses/qdlrs-BSD-3-Clause.txt")
        if name.startswith(("egui", "epaint", "emath", "ecolor")) or name == "eframe":
            bundled.append("licenses/egui-MIT.txt")
        if name == "rfd":
            bundled.append("licenses/rfd-MIT.txt")
        row = {
            "name": name, "version": version, "source": package["source"],
            "repository": package["repository"], "license": package["license"],
            "bundled_notices": bundled,
        }
        if notice := supplemental.get(f"{name}-{version}"):
            filename, sources = notice
            rel = f"licenses/cargo/upstream/{filename}"
            if not (destination / rel).exists():
                copy(upstream / filename, destination / rel)
            bundled.append(rel)
            row["upstream_notice_sources"] = sources
        rows.append(row)
    write_json(destination / INDEX, rows)


def checksum(reader):
    digest = hashlib.sha256()
    for data in iter(lambda: reader.read(1024 * 1024), b""):
        digest.update(data)
    return digest.hexdigest()


def file_checksum(path):
    with path.open("rb") as reader:
        return checksum(reader)


def stage(target, features, binaries, destination):
    version, packages = production_graph(target, features)
    _, _, cli, gui = TARGETS[target]
    destination.mkdir()
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", *GUIDES):
        copy(ROOT / name, destination / name)
    for name, source in LICENSES.items():
        copy(ROOT / source, destination / name)
    for name in (cli, gui):
        copy(binaries / PurePosixPath(name).name, destination / name)
        (destination / name).chmod(0o755)

    if target == "aarch64-apple-darwin":
        info = plistlib.loads((ROOT / "packaging/macos/Info.plist").read_bytes())
        info["CFBundleShortVersionString"] = re.split(r"[-+]", version, maxsplit=1)[0]
        (destination / PLIST).write_bytes(plistlib.dumps(info))
        # The linker's ad hoc signature covers the executable. Seal the assembled bundle.
        bundle = destination / MAC_APP
        subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", bundle], check=True)
        subprocess.run(["codesign", "--verify", "--strict", bundle], check=True)
    if target == "x86_64-unknown-linux-gnu":
        copy(ROOT / "packaging/linux/oplus-edl.desktop", destination / DESKTOP_ENTRY)

    dependency_notices(destination, packages)
    write_json(destination / BUILD, {
        "version": version, "target": target, "features": sorted(FEATURES),
        "cargo_lock_sha256": file_checksum(ROOT / "Cargo.lock"),
    })
    manifest = []
    for path in sorted(destination.rglob("*")):
        if path.is_file():
            manifest.append(f"{file_checksum(path)}  {path.relative_to(destination).as_posix()}\n")
    (destination / MANIFEST).write_text("".join(manifest), encoding="utf-8")
    print(version)


def verify_binary(reader, target, is_gui):
    header = reader.read(64)
    if target == "aarch64-apple-darwin":
        require(header[:4] == b"\xcf\xfa\xed\xfe", "expected thin Mach-O 64-bit")
        cpu, _, kind, ncmds, size = struct.unpack_from("<IIIII", header, 4)
        require(cpu == 0x0100000C and kind == 2, "expected executable Mach-O arm64")
        require(0 < size <= 1024 * 1024, "invalid Mach-O load command length")
        reader.seek(32)
        commands = reader.read(size)
        offset = 0
        versions = []
        for _ in range(ncmds):
            cmd, length = struct.unpack_from("<II", commands, offset)
            require(length >= 8 and offset + length <= len(commands), "invalid Mach-O load command")
            if cmd == 0x32:
                versions.append(struct.unpack_from("<II", commands, offset + 8))
            offset += length
        require(versions == [(1, 14 << 16)], "expected macOS 14.0 deployment target")
    elif target == "x86_64-pc-windows-msvc":
        require(header[:2] == b"MZ", "expected Windows PE")
        offset, = struct.unpack_from("<I", header, 60)
        require(64 <= offset <= 1024 * 1024, "invalid PE header offset")
        reader.seek(offset)
        pe = reader.read(94)
        require(pe[:6] == b"PE\0\0\x64\x86" and pe[24:26] == b"\x0b\x02", "expected Windows PE32+ x86_64")
        subsystem, = struct.unpack_from("<H", pe, 24 + 68)
        require(subsystem == (2 if is_gui else 3), "unexpected Windows executable subsystem")
    else:
        require(header[:7] == b"\x7fELF\x02\x01\x01", "expected little-endian ELF64")
        kind, cpu = struct.unpack_from("<HH", header, 16)
        require(cpu == 62 and kind in (2, 3), "expected executable ELF x86_64")


def archive_members(archive, is_zip):
    entries = archive.infolist() if is_zip else archive.getmembers()
    seen = set()
    for entry in entries:
        name = entry.filename if is_zip else entry.name
        path = PurePosixPath(name)
        require(path.parts and path.parts[0] == "oplus-edl" and ".." not in path.parts
                and "\\" not in name and name.rstrip("/") == path.as_posix(),
                f"unexpected archive path: {name}")
        require(path not in seen, f"duplicate archive path: {name}")
        seen.add(path)
        mode = entry.external_attr >> 16 if is_zip else entry.mode
        is_directory = entry.is_dir() if is_zip else entry.isdir()
        if is_directory:
            continue
        require(len(path.parts) > 1, f"invalid archive entry: {name}")
        is_regular = stat.S_IFMT(mode) in (0, stat.S_IFREG) if is_zip else entry.isfile()
        require(is_regular, f"archive entry is not a regular file: {name}")
        yield PurePosixPath(*path.parts[1:]).as_posix(), entry, mode


def verify(target, artifact):
    suffix, extension, cli, gui = TARGETS[target]
    is_zip = extension == "zip"
    open_archive = zipfile.ZipFile if is_zip else tarfile.open
    with open_archive(artifact, "r" if is_zip else "r:gz") as archive:
        open_file = archive.open if is_zip else archive.extractfile
        entries = {name: (entry, mode) for name, entry, mode in archive_members(archive, is_zip)}
        required = {"LICENSE", "THIRD_PARTY_NOTICES.md", *GUIDES, *LICENSES, BUILD, MANIFEST,
                    INDEX, BORINGSSL, cli, gui}
        if target == "aarch64-apple-darwin":
            required.update((PLIST, CODE_RESOURCES))
        if target == "x86_64-unknown-linux-gnu":
            required.add(DESKTOP_ENTRY)
        require(required <= entries.keys(), f"missing files: {sorted(required - entries.keys())}")
        require(all(name.startswith("licenses/cargo/") for name in entries.keys() - required),
                "unexpected payload outside the release layout")

        def read(name):
            with open_file(entries[name][0]) as stream:
                return stream.read()

        build = json.loads(read(BUILD))
        require(build["target"] == target and set(build["features"]) == FEATURES, "unexpected build profile")
        version = build["version"]
        require(artifact.name == f"oplus-edl-{version}-{suffix}.{extension}", "filename disagrees with build metadata")
        notices = json.loads(read(INDEX))
        require(notices and all(name in entries for row in notices for name in row["bundled_notices"]),
                "dependency notice index refers to a missing file")
        if PLIST in entries:
            info = plistlib.loads(read(PLIST))
            require(
                info["CFBundleExecutable"] == PurePosixPath(gui).name
                and info["CFBundleIdentifier"] == BUNDLE_ID
                and info["CFBundlePackageType"] == "APPL"
                and info["LSMinimumSystemVersion"] == "14.0"
                and info["CFBundleShortVersionString"] == re.split(r"[-+]", version, maxsplit=1)[0],
                "invalid macOS app metadata"
            )
        if DESKTOP_ENTRY in entries:
            config = configparser.ConfigParser(interpolation=None)
            config.read_string(read(DESKTOP_ENTRY).decode("utf-8"))
            entry = config["Desktop Entry"]
            require(entry["type"] == "Application" and entry["exec"] == entry["tryexec"] == PurePosixPath(gui).name
                    and entry["terminal"] == "false", "invalid Linux desktop entry")

        for name in (cli, gui):
            entry, mode = entries[name]
            require(target == "x86_64-pc-windows-msvc" or mode & 0o111, f"missing execute bits: {name}")
            with open_file(entry) as stream:
                verify_binary(stream, target, is_gui=name == gui)

        expected = {}
        for line in read(MANIFEST).decode("utf-8").splitlines():
            digest, delimiter, name = line.partition("  ")
            require(delimiter and re.fullmatch(r"[0-9a-f]{64}", digest) and name not in expected,
                    "invalid checksum manifest")
            expected[name] = digest
        require(expected.keys() == entries.keys() - {MANIFEST}, "checksum manifest does not cover the payload")
        for name, (entry, _) in entries.items():
            if name != MANIFEST:
                with open_file(entry) as stream:
                    require(checksum(stream) == expected[name], f"checksum mismatch: {name}")

    print(f"Verified {artifact.name} ({target}, {len(entries)} files)")
    print(f"SHA-256 {file_checksum(artifact)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    stage_parser = commands.add_parser("stage")
    stage_parser.add_argument("target", choices=TARGETS)
    stage_parser.add_argument("features")
    stage_parser.add_argument("binaries", type=Path)
    stage_parser.add_argument("destination", type=Path)
    verify_parser = commands.add_parser("verify")
    verify_parser.add_argument("target", choices=TARGETS)
    verify_parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "stage":
            stage(args.target, args.features, args.binaries, args.destination)
        else:
            verify(args.target, args.archive)
    except (OSError, ValueError, KeyError, struct.error, tarfile.TarError, zipfile.BadZipFile,
            subprocess.CalledProcessError) as error:
        parser.exit(1, f"Packaging error: {error}\n")


if __name__ == "__main__":
    main()
