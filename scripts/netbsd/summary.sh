#!/usr/bin/env bash
# Print phases.tsv files as one markdown table, for the step summary.
#
#   summary.sh FILE...
#
# The columns are those that scripts/netbsd/phase.sh writes. Sizes are in MiB.

printf '| Phase | Exit | Time | Source | Objects | TOOLDIR | DESTDIR | RELEASEDIR | Free |\n'
printf '|---|---|---|---|---|---|---|---|---|\n'
cat "$@" | awk -F '\t' '
	{ printf "| %s | %s | %dh%02dm%02ds | %d | %d | %d | %d | %d | %d |\n",
		$1, $2, $3 / 3600, ($3 % 3600) / 60, $3 % 60, $4, $5, $6, $7, $8, $9 }'
