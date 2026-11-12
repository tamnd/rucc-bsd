#!/usr/bin/env python3
"""Bisect a regression of netbsd-r to a rucc commit (milestone B12, plan 16.13).

    bisect.py find
    bisect.py step

bisect.yml runs this script. The environment gives the inputs: GOOD, BAD, FIELDS, CONFIRMED,
WIDTH, PROBES and REPORT, as the inputs of bisect.yml describe them. GH_REPO is the repository of
the harness, GITHUB_REF_NAME the branch of the workflows, and GITHUB_RUN_ID the run of bisect.yml.

find looks at the two newest nightly runs of netbsd-r (netbsd-nightly.yml). When the newest one
failed and the one before it passed with another rucc commit, it starts bisect.yml with these two
commits, unless a bisect.yml run of the same bad commit exists.

step does one round of the search. The first round runs netbsd-r with the good commit and with
the bad commit, both with the inputs of the bad night. When the good commit fails, the cause is
not in rucc, and the round opens an issue. When the bad commit passes, the night was a flake, and
the search stops. Each later round runs netbsd-r with up to WIDTH commits between the good and
the bad commit at the same time, and then starts bisect.yml again with the smaller range. When
only the bad commit is left, it is the first bad commit, and the round opens an issue for it (or
comments on REPORT). A round that comes near the time limit of its job starts bisect.yml again
with PROBES, so that the next round waits for the same probe runs.
"""
import json
import os
import re
import subprocess
import sys
import time

RUCC = "tamnd/rucc"
PROBE = "netbsd-r.yml"
# The inputs that netbsd-nightly.yml gives netbsd-r, except rucc and g_run.
NIGHT_FIELDS = "build=kernel atf=true modules=true"
NIGHT = re.compile(r"^netbsd-r kernel trunk rucc=([0-9a-f]{40}) g_run=([0-9]+)$")
# The job of bisect.yml has 350 minutes. A round that waits longer than this hands over.
DEADLINE = 320 * 60
POLL = 120


def gh(*args, check=True):
    out = subprocess.run(["gh", *args], capture_output=True, text=True)
    if check and out.returncode != 0:
        sys.exit(f"gh {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


def summary(text):
    print(text)
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a") as f:
            f.write(text + "\n")


def run_url(run):
    return f"https://github.com/{os.environ['GH_REPO']}/actions/runs/{run}"


def commits(good, bad):
    """The commits after good up to bad, oldest first, as (sha, subject)."""
    data = json.loads(gh("api", f"repos/{RUCC}/compare/{good}...{bad}"))
    if data["status"] != "ahead":
        sys.exit(f"{bad} is not a descendant of {good} (compare status {data['status']})")
    if data["total_commits"] > len(data["commits"]):
        sys.exit(f"{data['total_commits']} commits from {good} to {bad}, more than compare lists")
    return [(c["sha"], c["commit"]["message"].splitlines()[0]) for c in data["commits"]]


def points(n, width):
    """The indexes to probe in a list of n commits whose last one is bad."""
    p = min(width, n - 1)
    return sorted({(i + 1) * n // (p + 1) - 1 for i in range(p)})


def fields_args(fields):
    args = []
    for field in fields.split():
        key, sep, _ = field.partition("=")
        if not sep or key in ("rucc", "bisect"):
            sys.exit(f"bad field {field!r} in FIELDS")
        args += ["-f", field]
    return args


def dispatch(sha, fields):
    me = os.environ["GITHUB_RUN_ID"]
    ref = os.environ["GITHUB_REF_NAME"]
    out = gh("workflow", "run", PROBE, "-r", ref, "-f", f"rucc={sha}", "-f", f"bisect={me}",
             *fields_args(fields))
    m = re.search(r"/actions/runs/([0-9]+)", out)
    if m:
        return m.group(1)
    want = f"rucc={sha} "
    for _ in range(20):
        time.sleep(15)
        runs = json.loads(gh("run", "list", "-w", PROBE, "-e", "workflow_dispatch", "-L", "30",
                             "--json", "databaseId,displayTitle"))
        for r in runs:
            t = r["displayTitle"]
            if want in t and t.endswith(f" bisect={me}"):
                return str(r["databaseId"])
    sys.exit(f"cannot find the netbsd-r run of rucc {sha}")


def wait(probes, start):
    """Wait for the probe runs. Return {sha: conclusion}, or None at the deadline."""
    while True:
        done = {}
        for sha, run in probes.items():
            r = json.loads(gh("run", "view", run, "--json", "status,conclusion"))
            if r["status"] == "completed":
                done[sha] = r["conclusion"]
        if len(done) == len(probes):
            return done
        if time.time() - start > DEADLINE:
            return None
        time.sleep(POLL)


def again(good, bad, fields, confirmed, width, report, probes=""):
    ref = os.environ["GITHUB_REF_NAME"]
    gh("workflow", "run", "bisect.yml", "-r", ref, "-f", f"good={good}", "-f", f"bad={bad}",
       "-f", f"fields={fields}", "-f", f"confirmed={str(confirmed).lower()}",
       "-f", f"width={width}", "-f", f"probes={probes}", "-f", f"report={report}")


def tell(title, body, report, labels):
    if report:
        gh("issue", "comment", report, "--body", f"**{title}**\n\n{body}")
        summary(f"Commented on #{report}.")
        return
    out = gh("issue", "create", "--title", title, "--body", body,
             *[a for label in labels for a in ("--label", label)])
    summary(f"Opened {out.strip()}.")


def step():
    good, bad = os.environ["GOOD"], os.environ["BAD"]
    fields = os.environ.get("FIELDS", "")
    confirmed = os.environ.get("CONFIRMED", "false") == "true"
    width = max(1, int(os.environ.get("WIDTH") or 3))
    report = os.environ.get("REPORT", "")
    labels = ["kind/bug", "sys/netbsd", "row/amd64"]
    start = time.time()
    line = commits(good, bad)
    subject = dict(line)
    summary(f"## bisect {good[:12]}..{bad[:12]}\n\n{len(line)} rucc commits, inputs `{fields}`.\n")

    if confirmed and len(line) == 1:
        sha = line[0][0]
        body = (f"netbsd-r with the inputs `{fields}` passes with rucc {good} and fails with rucc "
                f"{sha}, the next commit:\n\n"
                f"https://github.com/{RUCC}/commit/{sha} {subject[sha]}\n\n"
                f"Found by {run_url(os.environ['GITHUB_RUN_ID'])}.")
        summary(body)
        tell(f"netbsd-r regression at rucc {sha[:12]}", body, report, labels)
        return

    if os.environ.get("PROBES"):
        probes = dict(p.split(":") for p in os.environ["PROBES"].split(","))
    else:
        if confirmed:
            shas = [line[i][0] for i in points(len(line), width)]
        else:
            shas = [good, bad]
        probes = {sha: dispatch(sha, fields) for sha in shas}
    for sha, run in probes.items():
        summary(f"- probe rucc {sha[:12]}: {run_url(run)}")

    result = wait(probes, start)
    if result is None:
        summary("\nThe probes did not end in time. The next bisect.yml run waits for them.")
        again(good, bad, fields, confirmed, width, report,
              ",".join(f"{s}:{r}" for s, r in probes.items()))
        return
    for sha, conclusion in result.items():
        summary(f"- rucc {sha[:12]}: {conclusion}")
    odd = {s: c for s, c in result.items() if c not in ("success", "failure")}
    if odd:
        sys.exit(f"probe runs that neither passed nor failed: {odd}")

    if not confirmed:
        if result[good] == "failure":
            body = (f"netbsd-r with the inputs `{fields}` fails with rucc {good} as well as with "
                    f"rucc {bad}. So the cause is not a rucc commit. It can be the pin, the G "
                    f"run or the harness.\n\nProbes: {run_url(probes[good])} and "
                    f"{run_url(probes[bad])}.")
            summary(body)
            tell(f"netbsd-r fails with rucc {good[:12]} and {bad[:12]}", body, report, labels)
            return
        if result[bad] == "success":
            summary(f"rucc {bad[:12]} passes this time, so the failed night was a flake.")
            return
        again(good, bad, fields, True, width, report)
        return

    index = {sha: i for i, (sha, _) in enumerate(line)}
    order = sorted(result, key=index.get)
    failed = [s for s in order if result[s] == "failure"]
    new_bad = failed[0] if failed else bad
    passed = [s for s in order if result[s] == "success" and index[s] < index[new_bad]]
    new_good = passed[-1] if passed else good
    late = [s for s in order if result[s] == "success" and index[s] > index[new_bad]]
    if late:
        summary(f"\nrucc {', '.join(s[:12] for s in late)} passed after a failed probe. "
                "The failure may be flaky. The search goes on with the first failure.")
    summary(f"\nNext range: {new_good[:12]}..{new_bad[:12]}.")
    again(new_good, new_bad, fields, True, width, report)


def find():
    repo = os.environ["GH_REPO"]
    runs = json.loads(gh("api", f"repos/{repo}/actions/workflows/{PROBE}/runs?event=workflow_dispatch"
                                "&branch=main&status=completed&per_page=60"))["workflow_runs"]
    nights = []
    for r in runs:
        m = NIGHT.match(r["display_title"])
        if m and r["actor"]["login"] == "github-actions[bot]" and r["conclusion"] in ("success", "failure"):
            nights.append((m.group(1), m.group(2), r["conclusion"], r["id"]))
    if len(nights) < 2:
        summary("Fewer than two nightly netbsd-r runs with a rucc commit in the title.")
        return
    (bad, g_run, last, last_run), (good, _, before, before_run) = nights[0], nights[1]
    summary(f"Newest night: rucc {bad[:12]}, {last}, {run_url(last_run)}.\n"
            f"The night before: rucc {good[:12]}, {before}, {run_url(before_run)}.")
    if last != "failure" or before != "success" or good == bad:
        summary("No new regression.")
        return
    titles = json.loads(gh("run", "list", "-w", "bisect.yml", "-L", "60", "--json", "displayTitle"))
    if any(bad in t["displayTitle"] for t in titles):
        summary(f"A bisect.yml run of rucc {bad[:12]} exists already.")
        return
    again(good, bad, f"{NIGHT_FIELDS} g_run={g_run}", False, 3, "")
    summary(f"Started bisect.yml for rucc {good[:12]}..{bad[:12]}.")


if __name__ == "__main__":
    if sys.argv[1:] == ["find"]:
        find()
    elif sys.argv[1:] == ["step"]:
        step()
    else:
        sys.exit(__doc__)
