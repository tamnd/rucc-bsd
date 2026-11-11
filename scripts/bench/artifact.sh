#!/usr/bin/env bash
# Download the newest artifact NAME of a run on main that has not expired (bench.yml).
#
#   artifact.sh NAME DIR
#
# The artifact can come from any workflow and any run that made it, also one whose later jobs
# failed. It prints "NAME: run RUN_ID of WORKFLOW, made AT" for the step summary. It needs
# GH_TOKEN and GITHUB_REPOSITORY.
set -euo pipefail
name=$1
dir=$2
read -r run made < <(gh api "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$name&per_page=50" \
	-q '[.artifacts[] | select(.expired | not) | select(.workflow_run.head_branch == "main")]
	    | sort_by(.created_at) | reverse | .[0] // empty | "\(.workflow_run.id) \(.created_at)"') || true
if [ -z "${run:-}" ]; then
	echo "no artifact $name of a run on main that has not expired" >&2
	exit 1
fi
workflow=$(gh run view "$run" -R "$GITHUB_REPOSITORY" --json workflowName -q .workflowName)
mkdir -p "$dir"
gh run download "$run" -R "$GITHUB_REPOSITORY" -n "$name" -D "$dir"
echo "$name: run $run of $workflow, made $made"
