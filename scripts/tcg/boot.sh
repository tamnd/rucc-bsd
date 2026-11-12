#!/usr/bin/env bash
# rb boot for a tcg job, made again when the kernel panics.
#
#   boot.sh OUT SECONDS RB [ARG...]
#
# It runs "RB boot ARG... --out OUT --timeout T", where T is the part of SECONDS that is left, and
# appends the output to the step summary. Under TCG on the runner, the stock G kernels also panic
# at times in ways that do not repeat: a fault on a nofault entry in vm_fault_lookup on FreeBSD
# aarch64, and "softints stuck for 16 seconds" on NetBSD aarch64 (tamnd/rucc-bsd#206). So when the
# boot ends in a panic, the script boots again with a new OUT, at most two more times, while at
# least 30 minutes of SECONDS are left. It does this for G and R alike. Each boot that panicked
# goes to OUT/panic-N, and the summary names each panic and the case that was running. The exit
# status is that of the last boot.
#
# RB_SUMMARY is the file of the summary, by default $GITHUB_STEP_SUMMARY. The host tests use a
# stub for RB.
set -uo pipefail

out=$1
seconds=$2
rb=$3
shift 3
summary=${RB_SUMMARY:-${GITHUB_STEP_SUMMARY:-/dev/null}}
start=$(date +%s)
min_left=${RB_MIN_LEFT:-1800}

left() {
	echo $((seconds - ($(date +%s) - start)))
}

# The panic and the case of the boot in OUT, from its boot.json, or nothing when it did not panic.
panic_of() {
	python3 -I - "$1/boot.json" <<'EOF'
import json, sys
try:
    with open(sys.argv[1], encoding="utf-8") as f:
        boot = json.load(f)
except (OSError, ValueError):
    sys.exit(0)
if boot.get("result") == "panic":
    seen = boot.get("seen") or {}
    print(f"{seen.get('panic') or 'a panic'}, at {seen.get('panic_case') or 'no test case'}")
EOF
}

tries=0
while :; do
	status=0
	"$rb" boot "$@" --out "$out" --timeout "$(left)" | tee -a "$summary" || status=$?
	if [ "$status" -eq 0 ]; then
		break
	fi
	panic=$(panic_of "$out")
	if [ -z "$panic" ] || [ "$tries" -ge 2 ]; then
		break
	fi
	if [ "$(left)" -lt "$min_left" ]; then
		echo "The kernel panicked: $panic. Only $(left) s are left, so the boot is not made again." |
			tee -a "$summary"
		break
	fi
	tries=$((tries + 1))
	echo "The kernel panicked: $panic. The boot is made again ($((tries + 1)) of at most 3), with $(left) s left." |
		tee -a "$summary"
	rm -rf "$out.panic-$tries"
	mv "$out" "$out.panic-$tries"
done

for n in $(seq 1 "$tries"); do
	mkdir -p "$out"
	mv "$out.panic-$n" "$out/panic-$n"
done
if [ "$tries" -gt 0 ]; then
	if [ "$status" -eq 0 ]; then
		echo "The boot passed after $tries panicked boots. They are in panic-1 to panic-$tries of the artifact." |
			tee -a "$summary"
	else
		echo "The boot failed after $tries panicked boots. They are in panic-1 to panic-$tries of the artifact." |
			tee -a "$summary"
	fi
fi
exit "$status"
