#!/usr/bin/env python3
"""End-to-end check of bucket versioning and Object Lock against a running Garage.

This talks to the S3 API with boto3, i.e. with a client that is completely
independent from the one the Rust integration tests use, so it also checks that
what Garage puts on the wire is what an ordinary S3 client expects.

The buckets it is given must already exist and be readable and writable by the
credentials it is given; `test-versioning.sh` sets that up. It is a separate
program so that it can also be pointed at any other running cluster:

    AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \\
    ENDPOINT=http://127.0.0.1:3911 python3 script/test-versioning.py
"""

import os
import sys
import time
from datetime import datetime, timedelta, timezone

# Match the checksum behaviour the CI shell sets, so that a local run and a CI
# run send the same requests
os.environ.setdefault("AWS_REQUEST_CHECKSUM_CALCULATION", "when_required")
os.environ.setdefault("AWS_RESPONSE_CHECKSUM_VALIDATION", "when_required")

import boto3
from botocore.exceptions import ClientError

ENDPOINT = os.environ.get("ENDPOINT", "http://127.0.0.1:3911")
REGION = os.environ.get("AWS_DEFAULT_REGION", "garage")
VERSIONING_BUCKET = os.environ.get("VERSIONING_BUCKET", "versioning-test")
OBJECT_LOCK_BUCKET = os.environ.get("OBJECT_LOCK_BUCKET", "objectlock-test")

KEY = "the/object"

failures = []


def check(name, condition, detail=""):
    if condition:
        print(f"  ok    {name}")
    else:
        print(f"  FAIL  {name} {detail}")
        failures.append(name)


def denied(name, fn):
    """Check that an operation is refused by the server"""
    try:
        fn()
    except ClientError as e:
        code = e.response["Error"]["Code"]
        status = e.response["ResponseMetadata"]["HTTPStatusCode"]
        print(f"  ok    {name} (refused with {code}, HTTP {status})")
        return
    print(f"  FAIL  {name} (the operation was allowed)")
    failures.append(name)


def body_of(response):
    return response["Body"].read()


def versions_of(s3, bucket, key):
    res = s3.list_object_versions(Bucket=bucket, Prefix=key)
    return res.get("Versions", []), res.get("DeleteMarkers", [])


def test_versioning(s3):
    print(f"\n== Bucket versioning ({VERSIONING_BUCKET}) ==")
    bucket = VERSIONING_BUCKET

    res = s3.get_bucket_versioning(Bucket=bucket)
    check(
        "a fresh bucket reports no versioning status",
        "Status" not in res,
        res.get("Status", ""),
    )

    s3.put_bucket_versioning(
        Bucket=bucket, VersioningConfiguration={"Status": "Enabled"}
    )
    res = s3.get_bucket_versioning(Bucket=bucket)
    check("versioning can be enabled", res.get("Status") == "Enabled", res)

    v1 = s3.put_object(Bucket=bucket, Key=KEY, Body=b"v1")["VersionId"]
    v2 = s3.put_object(Bucket=bucket, Key=KEY, Body=b"v2")["VersionId"]
    check("two writes create two distinct versions", v1 != v2, f"{v1} {v2}")

    check(
        "each version can be read back by its id",
        body_of(s3.get_object(Bucket=bucket, Key=KEY, VersionId=v1)) == b"v1"
        and body_of(s3.get_object(Bucket=bucket, Key=KEY, VersionId=v2)) == b"v2",
    )
    check(
        "reading without a version id gives the most recent one",
        body_of(s3.get_object(Bucket=bucket, Key=KEY)) == b"v2",
    )

    versions, markers = versions_of(s3, bucket, KEY)
    check("both versions are listed", len(versions) == 2, versions)
    check(
        "the most recent version is listed first and marked as latest",
        versions[0]["VersionId"] == v2 and versions[0]["IsLatest"] is True,
        versions[0] if versions else None,
    )
    check(
        "the older version is not marked as latest",
        versions[1]["VersionId"] == v1 and versions[1]["IsLatest"] is False,
        versions[1] if len(versions) > 1 else None,
    )
    check("no delete marker yet", markers == [], markers)

    listed = s3.list_objects_v2(Bucket=bucket, Prefix=KEY)
    check(
        "ListObjects reports the key once, not once per version",
        listed.get("KeyCount") == 1,
        listed.get("KeyCount"),
    )

    res = s3.delete_object(Bucket=bucket, Key=KEY)
    marker = res.get("VersionId")
    check("deleting the key creates a delete marker", res.get("DeleteMarker") is True, res)

    denied(
        "the object is hidden by the delete marker",
        lambda: s3.get_object(Bucket=bucket, Key=KEY),
    )

    versions, markers = versions_of(s3, bucket, KEY)
    check("the versions are kept behind the delete marker", len(versions) == 2, versions)
    check(
        "the delete marker is listed and is the latest version",
        len(markers) == 1
        and markers[0]["VersionId"] == marker
        and markers[0]["IsLatest"] is True,
        markers,
    )
    check(
        "a hidden version is still readable by its id",
        body_of(s3.get_object(Bucket=bucket, Key=KEY, VersionId=v2)) == b"v2",
    )

    s3.delete_object(Bucket=bucket, Key=KEY, VersionId=marker)
    check(
        "deleting the delete marker restores the object",
        body_of(s3.get_object(Bucket=bucket, Key=KEY)) == b"v2",
    )

    s3.delete_object(Bucket=bucket, Key=KEY, VersionId=v2)
    denied(
        "a version deleted by its id is gone",
        lambda: s3.get_object(Bucket=bucket, Key=KEY, VersionId=v2),
    )
    check(
        "the previous version becomes the current one again",
        body_of(s3.get_object(Bucket=bucket, Key=KEY)) == b"v1",
    )

    # ---- suspended versioning ----
    s3.put_bucket_versioning(
        Bucket=bucket, VersioningConfiguration={"Status": "Suspended"}
    )
    res = s3.get_bucket_versioning(Bucket=bucket)
    check("versioning can be suspended", res.get("Status") == "Suspended", res)

    n1 = s3.put_object(Bucket=bucket, Key=KEY, Body=b"n1")["VersionId"]
    n2 = s3.put_object(Bucket=bucket, Key=KEY, Body=b"n2")["VersionId"]
    check(
        "writes to a suspended bucket go to the null version",
        n1 == "null" and n2 == "null",
        f"{n1} {n2}",
    )

    versions, _ = versions_of(s3, bucket, KEY)
    ids = sorted(v["VersionId"] for v in versions)
    check(
        "the null version replaces itself and the older version is kept",
        ids == sorted([v1, "null"]),
        ids,
    )
    check(
        "the version written before suspension is still readable",
        body_of(s3.get_object(Bucket=bucket, Key=KEY, VersionId=v1)) == b"v1",
    )
    check(
        "the current version is the last null one",
        body_of(s3.get_object(Bucket=bucket, Key=KEY)) == b"n2",
    )


def test_object_lock(s3):
    print(f"\n== Object Lock ({OBJECT_LOCK_BUCKET}) ==")
    bucket = OBJECT_LOCK_BUCKET

    denied(
        "Object Lock cannot be turned on without versioning",
        lambda: s3.put_object_lock_configuration(
            Bucket=bucket,
            ObjectLockConfiguration={"ObjectLockEnabled": "Enabled"},
        ),
    )

    s3.put_bucket_versioning(
        Bucket=bucket, VersioningConfiguration={"Status": "Enabled"}
    )
    s3.put_object_lock_configuration(
        Bucket=bucket,
        ObjectLockConfiguration={
            "ObjectLockEnabled": "Enabled",
            "Rule": {"DefaultRetention": {"Mode": "GOVERNANCE", "Days": 1}},
        },
    )

    res = s3.get_object_lock_configuration(Bucket=bucket)["ObjectLockConfiguration"]
    check("Object Lock is reported as enabled", res.get("ObjectLockEnabled") == "Enabled", res)
    check(
        "the default retention is reported back",
        res["Rule"]["DefaultRetention"]["Mode"] == "GOVERNANCE"
        and res["Rule"]["DefaultRetention"]["Days"] == 1,
        res.get("Rule"),
    )

    denied(
        "versioning cannot be suspended while Object Lock is on",
        lambda: s3.put_bucket_versioning(
            Bucket=bucket, VersioningConfiguration={"Status": "Suspended"}
        ),
    )

    # ---- default retention ----
    v = s3.put_object(Bucket=bucket, Key="default-retention", Body=b"x")["VersionId"]
    res = s3.get_object_retention(
        Bucket=bucket, Key="default-retention", VersionId=v
    )["Retention"]
    check(
        "a new object gets the retention the bucket applies by default",
        res["Mode"] == "GOVERNANCE",
        res,
    )

    # ---- governance retention ----
    until = datetime.now(timezone.utc) + timedelta(hours=1)
    gov = s3.put_object(
        Bucket=bucket,
        Key="governance",
        Body=b"x",
        ObjectLockMode="GOVERNANCE",
        ObjectLockRetainUntilDate=until,
    )["VersionId"]

    head = s3.head_object(Bucket=bucket, Key="governance", VersionId=gov)
    check(
        "the lock settings are reported on HeadObject",
        head.get("ObjectLockMode") == "GOVERNANCE"
        and head.get("ObjectLockRetainUntilDate") is not None,
        {k: v for k, v in head.items() if k.startswith("ObjectLock")},
    )

    denied(
        "a version retained in governance mode cannot be deleted",
        lambda: s3.delete_object(Bucket=bucket, Key="governance", VersionId=gov),
    )
    denied(
        "its retention cannot be shortened either",
        lambda: s3.put_object_retention(
            Bucket=bucket,
            Key="governance",
            VersionId=gov,
            Retention={
                "Mode": "GOVERNANCE",
                "RetainUntilDate": datetime.now(timezone.utc) + timedelta(minutes=1),
            },
        ),
    )

    # A delete marker can still be created, as in AWS S3
    s3.delete_object(Bucket=bucket, Key="governance")
    check(
        "a delete marker can still be created over a locked version",
        body_of(s3.get_object(Bucket=bucket, Key="governance", VersionId=gov)) == b"x",
    )

    s3.put_object_retention(
        Bucket=bucket,
        Key="governance",
        VersionId=gov,
        BypassGovernanceRetention=True,
        Retention={},
    )
    s3.delete_object(Bucket=bucket, Key="governance", VersionId=gov)
    denied(
        "governance retention can be bypassed to delete the version",
        lambda: s3.get_object(Bucket=bucket, Key="governance", VersionId=gov),
    )

    # ---- legal hold ----
    lh = s3.put_object(Bucket=bucket, Key="legal-hold", Body=b"x")["VersionId"]
    s3.put_object_legal_hold(
        Bucket=bucket, Key="legal-hold", VersionId=lh, LegalHold={"Status": "ON"}
    )
    res = s3.get_object_legal_hold(Bucket=bucket, Key="legal-hold", VersionId=lh)
    check("a legal hold is reported back", res["LegalHold"]["Status"] == "ON", res)

    denied(
        "a version under a legal hold cannot be deleted, even with bypass",
        lambda: s3.delete_object(
            Bucket=bucket,
            Key="legal-hold",
            VersionId=lh,
            BypassGovernanceRetention=True,
        ),
    )

    s3.put_object_legal_hold(
        Bucket=bucket, Key="legal-hold", VersionId=lh, LegalHold={"Status": "OFF"}
    )
    s3.delete_object(
        Bucket=bucket,
        Key="legal-hold",
        VersionId=lh,
        BypassGovernanceRetention=True,
    )
    check("the version can be deleted once the hold is lifted", True)

    # ---- compliance retention ----
    comp = s3.put_object(
        Bucket=bucket,
        Key="compliance",
        Body=b"x",
        ObjectLockMode="COMPLIANCE",
        ObjectLockRetainUntilDate=datetime.now(timezone.utc) + timedelta(hours=1),
    )["VersionId"]

    denied(
        "a version retained in compliance mode cannot be deleted by anyone",
        lambda: s3.delete_object(
            Bucket=bucket,
            Key="compliance",
            VersionId=comp,
            BypassGovernanceRetention=True,
        ),
    )
    denied(
        "its retention cannot be lifted, even with bypass",
        lambda: s3.put_object_retention(
            Bucket=bucket,
            Key="compliance",
            VersionId=comp,
            BypassGovernanceRetention=True,
            Retention={},
        ),
    )
    denied(
        "nor downgraded to governance mode",
        lambda: s3.put_object_retention(
            Bucket=bucket,
            Key="compliance",
            VersionId=comp,
            BypassGovernanceRetention=True,
            Retention={
                "Mode": "GOVERNANCE",
                "RetainUntilDate": datetime.now(timezone.utc) + timedelta(hours=2),
            },
        ),
    )

    later = datetime.now(timezone.utc) + timedelta(hours=2)
    s3.put_object_retention(
        Bucket=bucket,
        Key="compliance",
        VersionId=comp,
        Retention={"Mode": "COMPLIANCE", "RetainUntilDate": later},
    )
    res = s3.get_object_retention(Bucket=bucket, Key="compliance", VersionId=comp)
    check(
        "compliance retention can only be extended",
        res["Retention"]["RetainUntilDate"].replace(microsecond=0)
        == later.replace(microsecond=0),
        res["Retention"]["RetainUntilDate"],
    )
    check(
        "the protected data is still readable",
        body_of(s3.get_object(Bucket=bucket, Key="compliance", VersionId=comp)) == b"x",
    )


def main():
    s3 = boto3.client("s3", endpoint_url=ENDPOINT, region_name=REGION)

    print(f"Testing versioning and Object Lock against {ENDPOINT}")

    test_versioning(s3)
    test_object_lock(s3)

    print()
    if failures:
        print(f"{len(failures)} check(s) failed:")
        for name in failures:
            print(f"  - {name}")
        sys.exit(1)

    print("All versioning and Object Lock checks passed.")


if __name__ == "__main__":
    main()
