# shellcheck shell=bash
# Shared settings for the NetBSD scripts. Each script sources this file.
#
# Every path is absolute and the same in every job, because a TOOLDIR records absolute paths
# (nbmake-amd64 names the source and object directories). A job that restores the tools from the
# cache must therefore use the same NB_ROOT as the job that built them.

set -eu

NB_HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
NB_REPO=$(cd "$NB_HERE/../.." && pwd)

# shellcheck source=../../conf/netbsd/trunk.pin
. "$NB_REPO/conf/netbsd/${NB_PIN_FILE:-trunk.pin}"

NB_ROOT=${NB_ROOT:-/mnt/nb}
NB_SRC=${NB_SRC:-$NB_ROOT/src}
NB_OBJ=${NB_OBJ:-$NB_ROOT/obj}
NB_TOOLS=${NB_TOOLS:-$NB_ROOT/tools}
NB_DEST=${NB_DEST:-$NB_ROOT/destdir}
NB_REL=${NB_REL:-$NB_ROOT/release}
NB_LOGS=${NB_LOGS:-$NB_ROOT/logs}
NB_MACHINE=${NB_MACHINE:-amd64}
NB_JOBS=${NB_JOBS:-$(nproc)}

mkdir -p "$NB_LOGS"

# The build.sh command line every phase uses. -U is an unprivileged build, which writes a METALOG
# that the sets and the images need. -N1 prints one short line per command; NB_NOISE=2 prints the
# commands themselves, which the R builds use so that a failed compile shows its command line.
nb_buildsh() {
	(cd "$NB_SRC" && ./build.sh -U -N "${NB_NOISE:-1}" -m "$NB_MACHINE" -j "$NB_JOBS" \
		-O "$NB_OBJ" -T "${NB_TOOLDIR:-$NB_TOOLS}" -D "$NB_DEST" -R "$NB_REL" "$@")
}

# Print the size of a directory in MiB, or 0 when it does not exist.
nb_du() {
	if [ -d "$1" ]; then du -sxm "$1" 2>/dev/null | cut -f1; else echo 0; fi
}

# Append a line to the step summary when the script runs in GitHub Actions.
nb_summary() {
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then printf '%s\n' "$*" >> "$GITHUB_STEP_SUMMARY"; fi
}
