use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{
	BucketLifecycleConfiguration, BucketVersioningStatus, Delete, ExpirationStatus, LifecycleRule,
	LifecycleRuleFilter, NoncurrentVersionExpiration, ObjectIdentifier, VersioningConfiguration,
};

use crate::common;

const KEY: &str = "versioned-object";

fn versioning(status: BucketVersioningStatus) -> VersioningConfiguration {
	VersioningConfiguration::builder().status(status).build()
}

#[tokio::test]
async fn test_bucket_versioning_configuration() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-config");

	// A bucket on which versioning was never enabled has no status at all,
	// which is distinct from having versioning suspended.
	let r = ctx
		.client
		.get_bucket_versioning()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.status, None);

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Enabled))
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_bucket_versioning()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.status, Some(BucketVersioningStatus::Enabled));

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Suspended))
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_bucket_versioning()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.status, Some(BucketVersioningStatus::Suspended));
}

#[tokio::test]
async fn test_versioned_put_get_delete() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-objects");

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Enabled))
		.send()
		.await
		.unwrap();

	// Two writes to the same key create two distinct versions
	let mut version_ids = vec![];
	for body in [&b"v1"[..], &b"v2"[..]] {
		let r = ctx
			.client
			.put_object()
			.bucket(&bucket)
			.key(KEY)
			.body(ByteStream::from(body.to_vec()))
			.send()
			.await
			.unwrap();
		version_ids.push(r.version_id.unwrap());
	}
	assert_ne!(version_ids[0], version_ids[1]);

	// Both versions can be read back by their version id
	for (version_id, expected) in version_ids.iter().zip([&b"v1"[..], &b"v2"[..]]) {
		let o = ctx
			.client
			.get_object()
			.bucket(&bucket)
			.key(KEY)
			.version_id(version_id)
			.send()
			.await
			.unwrap();
		assert_eq!(o.version_id.as_ref(), Some(version_id));
		assert_bytes_eq!(o.body, expected);
	}

	// Reading without a version id gives the most recent one
	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v2");

	// Both versions are listed, the most recent one first
	let r = ctx
		.client
		.list_object_versions()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let versions = r.versions.unwrap();
	assert_eq!(versions.len(), 2);
	assert_eq!(versions[0].version_id.as_ref(), Some(&version_ids[1]));
	assert_eq!(versions[0].is_latest, Some(true));
	assert_eq!(versions[0].size, Some(2));
	assert_eq!(versions[1].version_id.as_ref(), Some(&version_ids[0]));
	assert_eq!(versions[1].is_latest, Some(false));
	assert!(r.delete_markers.is_none() || r.delete_markers.as_ref().unwrap().is_empty());

	// ListObjects only reports the current version of the object
	let r = ctx
		.client
		.list_objects_v2()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.key_count, Some(1));

	// Deleting the key adds a delete marker and hides the object
	let r = ctx
		.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_eq!(r.delete_marker, Some(true));
	let marker_id = r.version_id.unwrap();

	assert!(ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.is_err());

	let r = ctx
		.client
		.list_object_versions()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.versions.as_ref().unwrap().len(), 2);
	let markers = r.delete_markers.unwrap();
	assert_eq!(markers.len(), 1);
	assert_eq!(markers[0].version_id.as_ref(), Some(&marker_id));
	assert_eq!(markers[0].is_latest, Some(true));

	// ... but the previous versions are still readable by their version id
	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_ids[1])
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v2");

	// Deleting the delete marker restores the object
	ctx.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&marker_id)
		.send()
		.await
		.unwrap();

	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v2");

	// Deleting a version by its id removes it for good
	ctx.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_ids[1])
		.send()
		.await
		.unwrap();

	assert!(ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_ids[1])
		.send()
		.await
		.is_err());

	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v1");
}

#[tokio::test]
async fn test_suspended_versioning_keeps_existing_versions() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-suspended");

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Enabled))
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.put_object()
		.bucket(&bucket)
		.key(KEY)
		.body(ByteStream::from_static(b"v1"))
		.send()
		.await
		.unwrap();
	let v1 = r.version_id.unwrap();

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Suspended))
		.send()
		.await
		.unwrap();

	// Writes now go to the object's `null` version, which they replace
	for body in [&b"v2"[..], &b"v3"[..]] {
		let r = ctx
			.client
			.put_object()
			.bucket(&bucket)
			.key(KEY)
			.body(ByteStream::from(body.to_vec()))
			.send()
			.await
			.unwrap();
		assert_eq!(r.version_id.as_deref(), Some("null"));
	}

	// The version written while versioning was enabled is still there,
	// alongside a single `null` version
	let r = ctx
		.client
		.list_object_versions()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let mut listed = r
		.versions
		.unwrap()
		.into_iter()
		.map(|v| v.version_id.unwrap())
		.collect::<Vec<_>>();
	listed.sort();
	let mut expected = vec![v1.clone(), "null".to_string()];
	expected.sort();
	assert_eq!(listed, expected);

	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&v1)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v1");

	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v3");
}

#[tokio::test]
async fn test_delete_objects_with_version_ids() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-delete-objects");

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Enabled))
		.send()
		.await
		.unwrap();

	let mut version_ids = vec![];
	for body in [&b"v1"[..], &b"v2"[..]] {
		let r = ctx
			.client
			.put_object()
			.bucket(&bucket)
			.key(KEY)
			.body(ByteStream::from(body.to_vec()))
			.send()
			.await
			.unwrap();
		version_ids.push(r.version_id.unwrap());
	}

	// A batch delete that names a version deletes that version only
	ctx.client
		.delete_objects()
		.bucket(&bucket)
		.delete(
			Delete::builder()
				.objects(
					ObjectIdentifier::builder()
						.key(KEY)
						.version_id(&version_ids[0])
						.build()
						.unwrap(),
				)
				.build()
				.unwrap(),
		)
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.list_object_versions()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let versions = r.versions.unwrap();
	assert_eq!(versions.len(), 1);
	assert_eq!(versions[0].version_id.as_ref(), Some(&version_ids[1]));

	// A batch delete without a version id adds a delete marker
	let r = ctx
		.client
		.delete_objects()
		.bucket(&bucket)
		.delete(
			Delete::builder()
				.objects(ObjectIdentifier::builder().key(KEY).build().unwrap())
				.build()
				.unwrap(),
		)
		.send()
		.await
		.unwrap();
	let deleted = r.deleted.unwrap();
	assert_eq!(deleted.len(), 1);
	assert_eq!(deleted[0].delete_marker, Some(true));
	assert!(deleted[0].delete_marker_version_id.is_some());

	let r = ctx
		.client
		.list_object_versions()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.versions.as_ref().unwrap().len(), 1);
	assert_eq!(r.delete_markers.as_ref().unwrap().len(), 1);
}

#[tokio::test]
async fn test_noncurrent_version_expiration_configuration() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-lifecycle");

	ctx.client
		.put_bucket_lifecycle_configuration()
		.bucket(&bucket)
		.lifecycle_configuration(
			BucketLifecycleConfiguration::builder()
				.rules(
					LifecycleRule::builder()
						.id("expire-old-versions")
						.status(ExpirationStatus::Enabled)
						.filter(LifecycleRuleFilter::builder().prefix("data/").build())
						.noncurrent_version_expiration(
							NoncurrentVersionExpiration::builder()
								.noncurrent_days(30)
								.newer_noncurrent_versions(3)
								.build(),
						)
						.build()
						.unwrap(),
				)
				.build()
				.unwrap(),
		)
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_bucket_lifecycle_configuration()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let rules = r.rules.unwrap();
	assert_eq!(rules.len(), 1);
	let nve = rules[0].noncurrent_version_expiration.as_ref().unwrap();
	assert_eq!(nve.noncurrent_days, Some(30));
	assert_eq!(nve.newer_noncurrent_versions, Some(3));

	// A rule that would expire versions on the day they become noncurrent is
	// rejected, as in AWS S3
	assert!(ctx
		.client
		.put_bucket_lifecycle_configuration()
		.bucket(&bucket)
		.lifecycle_configuration(
			BucketLifecycleConfiguration::builder()
				.rules(
					LifecycleRule::builder()
						.status(ExpirationStatus::Enabled)
						.filter(LifecycleRuleFilter::builder().prefix("").build())
						.noncurrent_version_expiration(
							NoncurrentVersionExpiration::builder()
								.noncurrent_days(0)
								.build(),
						)
						.build()
						.unwrap(),
				)
				.build()
				.unwrap(),
		)
		.send()
		.await
		.is_err());
}

#[tokio::test]
async fn test_copy_object_from_a_version() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("versioning-copy");

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(versioning(BucketVersioningStatus::Enabled))
		.send()
		.await
		.unwrap();

	let mut version_ids = vec![];
	for body in [&b"v1"[..], &b"v2"[..]] {
		let r = ctx
			.client
			.put_object()
			.bucket(&bucket)
			.key(KEY)
			.body(ByteStream::from(body.to_vec()))
			.send()
			.await
			.unwrap();
		version_ids.push(r.version_id.unwrap());
	}

	ctx.client
		.copy_object()
		.bucket(&bucket)
		.key("copy-of-v1")
		.copy_source(format!("{}/{}?versionId={}", bucket, KEY, version_ids[0]))
		.send()
		.await
		.unwrap();

	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key("copy-of-v1")
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, b"v1");
}
