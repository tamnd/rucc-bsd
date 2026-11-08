#!/usr/bin/env bash
# Fetch the pinned NetBSD tree into $NB_SRC.
#
# The abbreviated pin is resolved with the GitHub API and must give the full hash in the pin file.
# Then only that commit is fetched, with depth 1, and the checkout is checked against the hash.
#
# Every file in the tree gets the commit's time as its modification time. A git checkout gives
# each file the time of the checkout, which is newer than objects restored from a cache, so make
# would rebuild everything in each job. With one fixed time in every job, a restored object is
# newer than its sources, as it was in the job that built it.

. "$(dirname "$0")/common.sh"

full=$(gh api "repos/$NB_PIN_REPO/commits/$NB_PIN_SHORT" --jq .sha)
if [ "$full" != "$NB_PIN_COMMIT" ]; then
	echo "fetch: $NB_PIN_SHORT resolves to $full, but the pin file says $NB_PIN_COMMIT" >&2
	exit 1
fi

start=$(date +%s)
mkdir -p "$NB_SRC"
git -C "$NB_SRC" init -q
git -C "$NB_SRC" fetch -q --depth 1 "https://github.com/$NB_PIN_REPO" "$NB_PIN_COMMIT"
git -C "$NB_SRC" checkout -q FETCH_HEAD
head=$(git -C "$NB_SRC" rev-parse HEAD)
if [ "$head" != "$NB_PIN_COMMIT" ]; then
	echo "fetch: the checkout is $head, not $NB_PIN_COMMIT" >&2
	exit 1
fi

when=$(git -C "$NB_SRC" log -1 --format=%ct)
find "$NB_SRC" -path "$NB_SRC/.git" -prune -o -exec touch -h -d "@$when" {} +
end=$(date +%s)

printf 'fetch\t0\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n' $((end - start)) "$(nb_du "$NB_SRC")" \
	"$(nb_du "$NB_OBJ")" "$(nb_du "$NB_TOOLS")" "$(nb_du "$NB_DEST")" "$(nb_du "$NB_REL")" \
	"$(df -Pm "$NB_ROOT" | awk 'NR == 2 { print $4 }')" >> "$NB_LOGS/phases.tsv"
echo "fetch: $NB_PIN_REPO at $head in $((end - start)) s, $(nb_du "$NB_SRC") MiB"
