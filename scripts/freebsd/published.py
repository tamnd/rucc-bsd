#!/usr/bin/env python3
"""Get the test counts that FreeBSD's own CI published for one build of a test job.

    published.py JOB OUT.json [BUILD]

JOB is a ci.freebsd.org job, for example FreeBSD-main-amd64-test or FreeBSD-stable-15-amd64-test.
BUILD is a build number of the job, as conf/freebsd/published.conf gives for a pin. Without it, the
build is the job's last completed build.
The counts are those of the job's JUnit report from kyua report-junit. There, failed and broken cases
are both failures, and an expected failure is a pass. The CI test VM is bhyve with 2 vCPUs and 8 GB.
"""

import json
import sys
import urllib.request

API = (
    "https://ci.freebsd.org/job/{job}/{build}/api/json?tree=number,result,duration,timestamp,"
    "actions[totalCount,failCount,skipCount,lastBuiltRevision[SHA1]]"
)


def main():
    job, out = sys.argv[1], sys.argv[2]
    build = sys.argv[3] if len(sys.argv) > 3 else "lastCompletedBuild"
    with urllib.request.urlopen(API.format(job=job, build=build), timeout=60) as r:
        b = json.load(r)
    rec = {
        "job": job,
        "build": b["number"],
        "url": f"https://ci.freebsd.org/job/{job}/{b['number']}/",
        "result": b["result"],
        "minutes": round(b["duration"] / 60000),
        "timestamp": b["timestamp"] // 1000,
    }
    for a in b["actions"]:
        if "totalCount" in a:
            rec.update(total=a["totalCount"], failed=a["failCount"], skipped=a["skipCount"])
        if "lastBuiltRevision" in a:
            rec["commit"] = a["lastBuiltRevision"]["SHA1"]
    with open(out, "w") as f:
        json.dump(rec, f, indent=1, sort_keys=True)
        f.write("\n")
    print(json.dumps(rec, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
