+++
title = "S3 Compatibility status"
weight = 70
+++

## DISCLAIMER

**The compatibility list for other platforms is given only for informational
purposes and based on available documentation.** They are sometimes completed,
in a best effort approach, with the source code and inputs from maintainers
when documentation is lacking. We are not proactively monitoring new versions
of each software: check the modification history to know when the page has been
updated for the last time. Some entries will be inexact or outdated. For any
serious decision, you must make your own tests.
**The official documentation of each project can be accessed by clicking on the
project name in the column header.**

Feel free to open a PR to suggest fixes this table. Minio is missing because they do not provide a public S3 compatibility list.

## Update history

- 2022-02-07 - First version of this page
- 2022-05-25 - Many Ceph S3 endpoints are not documented but implemented. Following a notification from the Ceph community, we added them.


## High-level features

| Feature                      | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [signature v2](https://docs.aws.amazon.com/AmazonS3/latest/API/Appendix-Sigv2.html) (deprecated) | ❌ Missing | ✅ |  ✅ | ✅ | ✅ |
| [signature v4](https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-authenticating-requests.html) |  ✅ Implemented |  ✅ |  ✅ | ❌ | ✅ |
| [URL path-style](https://docs.aws.amazon.com/AmazonS3/latest/userguide/VirtualHosting.html#path-style-access) (eg. `host.tld/bucket/key`) |  ✅ Implemented | ✅ |  ✅ | ❓| ✅ |
| [URL vhost-style](https://docs.aws.amazon.com/AmazonS3/latest/userguide/VirtualHosting.html#virtual-hosted-style-access) URL (eg. `bucket.host.tld/key`) |  ✅ Implemented | ❌| ✅| ✅ | ✅ |
| [Presigned URLs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/ShareObjectPreSignedURL.html) |  ✅ Implemented | ❌|  ✅ | ✅ |  ✅(❓) |
| [SSE-C encryption](https://docs.aws.amazon.com/AmazonS3/latest/userguide/ServerSideEncryptionCustomerKeys.html) |  ✅ Implemented | ❓ |  ✅ | ❌ |  ✅ |
| [Bucket versioning](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Versioning.html) | ✅ Implemented | ✅ |  ✅ | ❌ | ✅ |
| [Object Lock](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html) | ✅ Implemented | ❌ |  ✅ | ❌ | ❌ |

*Note:* OpenIO does not says if it supports presigned URLs. Because it is part
of signature v4 and they claim they support it without additional precisions,
we suppose that OpenIO supports presigned URLs.


## Endpoint implementation

All endpoints that are missing on Garage will return a 501 Not Implemented.
Some `x-amz-` headers are not implemented.

### Core endpoints

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [CreateBucket](https://docs.aws.amazon.com/AmazonS3/latest/API/API_CreateBucket.html)                 | ✅ Implemented                      | ✅ | ✅ | ✅ | ✅ |
| [DeleteBucket](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucket.html)                 | ✅ Implemented                      | ✅ |  ✅ | ✅ | ✅ |
| [GetBucketLocation](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketLocation.html)            | ✅ Implemented                 | ✅ | ✅ | ❌ | ✅ |
| [HeadBucket](https://docs.aws.amazon.com/AmazonS3/latest/API/API_HeadBucket.html)                   | ✅ Implemented                      | ✅ | ✅ |  ✅ | ✅ |
| [ListBuckets](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBuckets.html)                  | ✅ Implemented                      | ❌| ✅ | ✅ | ✅ |
| [HeadObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_HeadObject.html)                   | ✅ Implemented                      | ✅ | ✅ | ✅ | ✅ |
| [CopyObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_CopyObject.html)                   | ✅ Implemented                      |  ✅ | ✅ | ✅ | ✅ |
| [DeleteObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObject.html)                 | ✅ Implemented                      | ✅ | ✅ | ✅ | ✅ |
| [DeleteObjects](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObjects.html)                | ✅ Implemented                      |  ✅  | ✅ | ✅ | ✅ |
| [GetObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html)                    | ✅ Implemented                      |  ✅ | ✅ | ✅ | ✅ |
| [ListObjects](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjects.html)                  | ✅ Implemented (see details below)   | ✅ | ✅ |  ✅ | ❌|
| [ListObjectsV2](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectsV2.html)                | ✅ Implemented                      | ❌|  ✅  | ❌| ✅ |
| [PostObject](https://docs.aws.amazon.com/AmazonS3/latest/API/RESTObjectPOST.html)                  | ✅ Implemented                      | ❌| ✅ | ❌| ❌|
| [PutObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObject.html)                    | ✅ Implemented                      | ✅ | ✅ | ✅ | ✅ |

**ListObjects:** Implemented, but there isn't a very good specification of what
`encoding-type=url` covers so there might be some encoding bugs. In our
implementation the url-encoded fields are in the same in ListObjects as they
are in ListObjectsV2.

*Note: Ceph API documentation is incomplete and lacks at least HeadBucket and UploadPartCopy,
but these endpoints are documented in [Red Hat Ceph Storage - Chapter 2. Ceph Object Gateway and the S3 API](https://access.redhat.com/documentation/en-us/red_hat_ceph_storage/4/html/developer_guide/ceph-object-gateway-and-the-s3-api)*

### Multipart Upload endpoints

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [AbortMultipartUpload](https://docs.aws.amazon.com/AmazonS3/latest/API/API_AbortMultipartUpload.html)         | ✅ Implemented  |  ✅ | ✅ | ✅ | ✅ |
| [CompleteMultipartUpload](https://docs.aws.amazon.com/AmazonS3/latest/API/API_CompleteMultipartUpload.html)   | ✅ Implemented  | ✅ | ✅ | ✅ | ✅ |
| [CreateMultipartUpload](https://docs.aws.amazon.com/AmazonS3/latest/API/API_CreateMultipartUpload.html)        | ✅ Implemented | ✅| ✅ | ✅ | ✅ |
| [ListMultipartUpload](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListMultipartUpload.html)          | ✅ Implemented   | ✅ | ✅ | ✅ | ✅ |
| [ListParts](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListParts.html)                    | ✅ Implemented             | ✅ | ✅ | ✅ | ✅ |
| [UploadPart](https://docs.aws.amazon.com/AmazonS3/latest/API/API_UploadPart.html)                  | ✅ Implemented             | ✅ | ✅| ✅ | ✅ |
| [UploadPartCopy](https://docs.aws.amazon.com/AmazonS3/latest/API/API_UploadPartCopy.html)               | ✅ Implemented        | ✅ | ✅ | ✅ | ✅ |

### Website endpoints

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketWebsite](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketWebsite.html)          | ✅ Implemented                      | ❌| ❌| ❌| ❌|
| [GetBucketWebsite](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketWebsite.html)             | ✅ Implemented                      |  ❌ | ❌| ❌| ❌|
| [PutBucketWebsite](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketWebsite.html)             | ⚠ Partially implemented (see below)| ❌| ❌| ❌| ❌|
| [DeleteBucketCors](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketCors.html)             | ✅ Implemented                      |  ❌|  ✅ | ❌| ✅ |
| [GetBucketCors](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketCors.html)                | ✅ Implemented                      |  ❌ |  ✅ | ❌| ✅ |
| [PutBucketCors](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketCors.html)                | ✅ Implemented                      | ❌|  ✅ | ❌| ✅ |

**PutBucketWebsite:** Implemented, but only stores the index document suffix and the error document path. Redirects are not supported.

*Note: Ceph radosgw has some support for static websites but it is different from the Amazon one. It also does not implement its configuration endpoints.*

### ACL, Policies endpoints

Amazon has 2 access control mechanisms in S3: ACL (legacy) and policies (new one).
Garage implements none of them, and has its own system instead, built around a per-access-key-per-bucket logic.
See Garage CLI reference manual to learn how to use Garage's permission system.

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketPolicy](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketPolicy.html) | ❌ Missing | ❌|  ✅ | ✅ | ❌|
| [GetBucketPolicy](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketPolicy.html) | ❌ Missing | ❌|  ✅ | ⚠ | ❌|
| [GetBucketPolicyStatus](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketPolicyStatus.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [PutBucketPolicy](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketPolicy.html) | ❌ Missing | ❌|  ✅ | ⚠ | ❌|
| [GetBucketAcl](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketAcl.html) | ❌ Missing | ✅ | ✅ | ✅ | ✅ |
| [PutBucketAcl](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketAcl.html) | ❌ Missing | ✅ | ✅ | ✅ | ✅ |
| [GetObjectAcl](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectAcl.html) | ❌ Missing | ✅ | ✅ | ✅ | ✅ |
| [PutObjectAcl](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectAcl.html) | ❌ Missing | ✅ | ✅ | ✅ | ✅ |

*Notes:* Riak CS only supports a subset of the policy configuration.

### Versioning, Lifecycle endpoints

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketLifecycle](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketLifecycle.html) | ✅ Implemented | ❌| ✅| ❌| ✅|
| [GetBucketLifecycleConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketLifecycleConfiguration.html) | ✅ Implemented | ❌| ✅ | ❌| ✅|
| [PutBucketLifecycleConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLifecycleConfiguration.html) | ⚠ Partially implemented (see below) | ❌| ✅ | ❌| ✅|
| [GetBucketVersioning](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketVersioning.html)          | ✅ Implemented       | ✅| ✅ | ❌| ✅|
| [ListObjectVersions](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectVersions.html) | ⚠ Partially implemented (see below) | ❌| ✅ | ❌| ✅|
| [PutBucketVersioning](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketVersioning.html) | ⚠ Partially implemented (see below) | ❌| ✅| ❌| ✅|

**PutBucketLifecycleConfiguration:** The supported actions are
`AbortIncompleteMultipartUpload`, `Expiration` (without the
`ExpiredObjectDeleteMarker` field) and `NoncurrentVersionExpiration` (with both
`NoncurrentDays` and `NewerNoncurrentVersions`). `NoncurrentVersionTransition`
and the remaining actions depend on storage classes, which Garage does not
implement. On a versioned bucket, `Expiration` adds a delete marker rather than
deleting the data, as the S3 API specifies, while `NoncurrentVersionExpiration`
permanently deletes the versions it applies to, except the ones that Object
Lock protects, which it leaves alone. As for `Expiration`, the days are counted
from the midnight that follows the day a version became noncurrent, i.e. the day
the version that replaced it was written. The deprecated `Prefix` member directly
in the the `Rule` structure/XML tag is not supported, specified prefixes must be
inside the `Filter` structure/XML tag.

**PutBucketVersioning:** MFA delete is not supported: a request that asks for
`MfaDelete` to be `Enabled` is rejected. As it changes how a bucket stores its
data, enabling or suspending versioning requires the `owner` permission on the
bucket, like the other bucket configuration endpoints.

**ListObjectVersions:** Unlike AWS S3, Garage does not interleave the `Version`
and `DeleteMarker` elements of the response: all versions come first, then all
delete markers. Both lists are individually sorted as the S3 API requires, which
is what SDKs that expose them as two separate lists (such as boto3) rely on.
The `Owner` element is not returned, as Garage does not implement object
ownership.

**Bucket versioning:** see the dedicated section below.

### Replication endpoints

Please open an issue if you have a use case for replication.

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketReplication](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketReplication.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [GetBucketReplication](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketReplication.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [PutBucketReplication](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketReplication.html) | ❌ Missing | ❌| ⚠ | ❌| ❌|

*Note: Ceph documentation briefly says that Ceph supports
[replication through the S3 API](https://docs.ceph.com/en/latest/radosgw/multisite-sync-policy/#s3-replication-api)
but with some limitations.
Additionally, replication endpoints are not documented in the S3 compatibility page so I don't know what kind of support we can expect.*

### Bucket versioning

Versioning is enabled per bucket, either with `PutBucketVersioning` or with
`garage bucket versioning --enable <bucket>`. Once enabled, it can only be
suspended, never disabled: the versions that were created while it was enabled
are kept until they are explicitly deleted.

When versioning is **enabled**, every write to a key creates a new version with
its own version id, and deleting a key adds a delete marker instead of removing
the data. Previous versions remain readable through `GetObject` with a
`versionId` parameter, and are listed by `ListObjectVersions`. Deleting a
specific version with `DELETE /key?versionId=...` removes it permanently and
frees the blocks it references.

When versioning is **suspended**, writes go to the object's `null` version,
which each write replaces, while the versions created while versioning was
enabled are kept.

Two behaviours differ from AWS S3, for backwards compatibility with the
versions of Garage that predate versioning support:

- On a bucket on which versioning was never enabled, `PutObject` keeps
  returning Garage's internal version uuid in the `x-amz-version-id` response
  header (AWS S3 returns no such header). On such buckets the `versionId` query
  parameter is ignored, so that a client that sends one of those uuids back
  keeps addressing the object rather than getting an error.
- `DELETE /key` on a key that does not exist does not create a delete marker;
  as in AWS S3, it is reported as a success.

Noncurrent versions are kept until they are explicitly deleted, or until a
`NoncurrentVersionExpiration` lifecycle rule expires them; see the note on
`PutBucketLifecycleConfiguration` above.

`garage bucket inspect-object <bucket> <key>` lists every version of an object
with its S3 version id and, when it has any, its Object Lock settings.

Object versions do not count as separate objects for the bucket's `max_objects`
quota, but the data of every version is counted in the bucket size and in the
`max_size` quota. On a bucket whose versioning is suspended, the `max_size`
check does not deduce the `null` version that a write replaces, so it errs on
the side of rejecting the write.

### Locking objects

Amazon defines a concept of [object locking](https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html) that can be achieved either through a Retention period or a Legal hold.

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [GetObjectLegalHold](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectLegalHold.html) | ✅ Implemented | ❌| ✅ | ❌| ❌|
| [PutObjectLegalHold](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectLegalHold.html) | ✅ Implemented | ❌| ✅ | ❌| ❌|
| [GetObjectRetention](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectRetention.html) | ✅ Implemented | ❌| ✅ | ❌| ❌|
| [PutObjectRetention](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectRetention.html) | ✅ Implemented | ❌| ✅ | ❌| ❌|
| [GetObjectLockConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectLockConfiguration.html) | ✅ Implemented | ❌| ✅ | ❌| ❌|
| [PutObjectLockConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectLockConfiguration.html) | ⚠ Partially implemented (see below) | ❌| ✅ | ❌| ❌|

Object Lock protects individual versions of objects, so it requires the bucket
to have versioning enabled. It can be turned on either at bucket creation, by
passing `x-amz-bucket-object-lock-enabled: true` to `CreateBucket` (which turns
versioning on as well), or afterwards on a bucket whose versioning is already
enabled, with `PutObjectLockConfiguration` or with
`garage bucket object-lock --enable <bucket>`. It can never be turned off, and
versioning cannot be suspended while it is on.

A version is protected either by a retention period or by a legal hold:

- a version under a **legal hold** cannot be deleted by anyone until the hold is
  lifted with `PutObjectLegalHold`;
- a version retained in **governance** mode can only be deleted, or have its
  retention shortened or lifted, by a caller that sends
  `x-amz-bypass-governance-retention: true`;
- a version retained in **compliance** mode cannot be deleted before its
  retain-until date by anyone, and its retention can only be extended. This
  holds for cluster administrators too: `garage bucket delete` refuses to delete
  a bucket that still holds retained versions, and `garage block purge` refuses
  to purge a locked version.

Deleting a locked version without naming it, i.e. `DELETE /key` with no
`versionId`, adds a delete marker and is always allowed, as in AWS S3: the
version itself is left untouched and can still be read by its version id.

The default retention of the bucket, if it has one, applies to every version
created without `x-amz-object-lock-mode` and
`x-amz-object-lock-retain-until-date` headers.

**PutObjectLockConfiguration:** Unlike AWS S3, Garage lets this endpoint turn
Object Lock on for an existing bucket, provided its versioning is already
enabled; AWS only allows it at bucket creation. Turning Object Lock off is never
possible.

**Bypassing governance retention:** AWS gates this on the
`s3:BypassGovernanceRetention` permission. Garage does not implement IAM
policies, so it requires the access key to have the `owner` permission on the
bucket instead.

### (Server-side) encryption

We think that you can either encrypt your server partition or do client-side encryption, so we did not implement server-side encryption for Garage.
Please open an issue if you have a use case.

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketEncryption](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketEncryption.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [GetBucketEncryption](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketEncryption.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [PutBucketEncryption](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketEncryption.html) | ❌ Missing | ❌| ✅ | ❌| ❌|

### Misc endpoints

| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [GetBucketNotificationConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketNotificationConfiguration.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [PutBucketNotificationConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketNotificationConfiguration.html) | ❌ Missing | ❌| ✅ | ❌| ❌|
| [DeleteBucketTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [GetBucketTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [PutBucketTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [DeleteObjectTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObjectTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [GetObjectTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [PutObjectTagging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObjectTagging.html) | ❌ Missing | ❌| ✅ | ❌| ✅ |
| [GetObjectTorrent](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectTorrent.html) | ❌ Missing | ❌| ✅ | ❌| ❌|

### Vendor specific endpoints

<details><summary>Display Amazon specific endpoints</summary>


| Endpoint                     | Garage                           | [Openstack Swift](https://docs.openstack.org/swift/latest/s3_compat.html) | [Ceph Object Gateway](https://docs.ceph.com/en/latest/radosgw/s3/) | [Riak CS](https://docs.riak.com/riak/cs/2.1.1/references/apis/storage/s3/index.html) | [OpenIO](https://docs.openio.io/latest/source/arch-design/s3_compliancy.html) |
|------------------------------|----------------------------------|-----------------|---------------|---------|-----|
| [DeleteBucketAnalyticsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketAnalyticsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [DeleteBucketIntelligentTieringConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketIntelligentTieringConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [DeleteBucketInventoryConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketInventoryConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [DeleteBucketMetricsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketMetricsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [DeleteBucketOwnershipControls](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteBucketOwnershipControls.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [DeletePublicAccessBlock](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeletePublicAccessBlock.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketAccelerateConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketAccelerateConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketAnalyticsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketAnalyticsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketIntelligentTieringConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketIntelligentTieringConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketInventoryConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketInventoryConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketLogging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketLogging.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketMetricsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketMetricsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketOwnershipControls](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketOwnershipControls.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetBucketRequestPayment](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketRequestPayment.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [GetPublicAccessBlock](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetPublicAccessBlock.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [ListBucketAnalyticsConfigurations](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBucketAnalyticsConfigurations.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [ListBucketIntelligentTieringConfigurations](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBucketIntelligentTieringConfigurations.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [ListBucketInventoryConfigurations](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBucketInventoryConfigurations.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [ListBucketMetricsConfigurations](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBucketMetricsConfigurations.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketAccelerateConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketAccelerateConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketAnalyticsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketAnalyticsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketIntelligentTieringConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketIntelligentTieringConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketInventoryConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketInventoryConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketLogging](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketLogging.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketMetricsConfiguration](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketMetricsConfiguration.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketOwnershipControls](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketOwnershipControls.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutBucketRequestPayment](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketRequestPayment.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [PutPublicAccessBlock](https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutPublicAccessBlock.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [RestoreObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_RestoreObject.html) | ❌ Missing | ❌| ❌| ❌| ❌|
| [SelectObjectContent](https://docs.aws.amazon.com/AmazonS3/latest/API/API_SelectObjectContent.html) | ❌ Missing | ❌| ❌| ❌| ❌|

</details>
