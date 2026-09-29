#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.
#
# Meson custom_target wrapper around cargo, adapted from DCV-viewer-gtk's
# build-aux/cargo.py. Runs `cargo <command>` for the servo-gtk crate, honoring
# the Meson buildtype, and copies the requested output artifact(s) into the
# Meson build directory.

import os
import shutil
import subprocess
import sys

# The cargo command to run: build, test, clippy, fmt.
command = sys.argv[1]
# The intermediate directory for the build (cargo --target-dir), or "unknown".
build_dir = sys.argv[2]
# The directory containing Cargo.toml.
source_dir = sys.argv[3]
# The Meson buildtype: debug, debugoptimized, release, ... or "unknown".
buildtype = sys.argv[4].lower()
# Comma-separated features, or "unknown" for defaults.
features = sys.argv[5].lower()
# Target triple, or "unknown" for the default host.
target = sys.argv[6]
# Remaining args: wanted output files and/or extra cargo flags (starting --).
output_and_flags = sys.argv[7:]

output = []
cargo_extra_flags = []
i = 0
while i < len(output_and_flags):
    arg = output_and_flags[i]
    if arg.startswith("--"):
        cargo_extra_flags.append(arg)
        if arg in ["--bin", "--example", "--test", "--bench"]:
            i += 1
            if i < len(output_and_flags):
                cargo_extra_flags.append(output_and_flags[i])
    else:
        output.append(arg)
    i += 1

if target == "unknown":
    target = os.environ.get("CARGO_BUILD_TARGET", default=target)

cmd = [
    "cargo",
    command,
    "--manifest-path",
    os.path.join(source_dir, "Cargo.toml"),
]

if build_dir != "unknown":
    cmd += ["--target-dir", build_dir]

if features != "unknown":
    cmd += ["--features", features]

if target != "unknown":
    cmd += ["--target={}".format(target)]

# Map Meson buildtypes to cargo profiles. Anything that is not a plain debug
# build maps to --release so optimized Meson builds produce optimized Rust.
if buildtype not in ("unknown", "debug", "plain"):
    cmd += ["--release"]

cmd += cargo_extra_flags

print('Calling cargo with "{}"'.format(cmd), flush=True)

process = subprocess.run(cmd, shell=False, stderr=sys.stderr, stdout=sys.stdout)

if process.returncode == 0 and command == "build":
    profile_dir = build_dir
    if target != "unknown":
        profile_dir = os.path.join(profile_dir, target)
    if buildtype in ("debug", "plain"):
        profile_dir = os.path.join(profile_dir, "debug")
    else:
        profile_dir = os.path.join(profile_dir, "release")

    for o in output:
        filename = os.path.basename(o)
        shutil.copy(os.path.join(profile_dir, filename), o)

sys.exit(process.returncode)

# ex: set ts=4 sw=4 et fenc=utf-8 syntax=python :
