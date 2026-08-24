use std::time::{SystemTime, UNIX_EPOCH};

use aws_sdk_s3::primitives::{ByteStream, DateTime};
use aws_sdk_s3::types::{
	BucketVersioningStatus, DefaultRetention, ObjectLockConfiguration, ObjectLockEnabled,
	ObjectLockLegalHold, ObjectLockLegalHoldStatus, ObjectLockMode, ObjectLockRetention,
	ObjectLockRetentionMode, ObjectLockRule, VersioningConfiguration,
};

use crate::common;
use crate::common::ext::CommandExt;

const KEY: &str = "locked-object";
const BODY: &[u8] = b"the data that must not be lost";

fn in_the_future(secs: i64) -> DateTime {
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap()
		.as_millis() as i64;
	DateTime::from_millis(now + secs * 1000)
}

/// Create a bucket with versioning and Object Lock enabled, optionally with a
/// default retention rule
async fn locked_bucket(
	ctx: &common::Context,
	name: &str,
	default_retention: Option<DefaultRetention>,
) -> String {
	let bucket = ctx.create_bucket(name);

	ctx.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(
			VersioningConfiguration::builder()
				.status(BucketVersioningStatus::Enabled)
				.build(),
		)
		.send()
		.await
		.unwrap();

	ctx.client
		.put_object_lock_configuration()
		.bucket(&bucket)
		.object_lock_configuration(
			ObjectLockConfiguration::builder()
				.object_lock_enabled(ObjectLockEnabled::Enabled)
				.set_rule(
					default_retention
						.map(|dr| ObjectLockRule::builder().default_retention(dr).build()),
				)
				.build(),
		)
		.send()
		.await
		.unwrap();

	bucket
}

async fn put_object(ctx: &common::Context, bucket: &str) -> String {
	ctx.client
		.put_object()
		.bucket(bucket)
		.key(KEY)
		.body(ByteStream::from_static(BODY))
		.send()
		.await
		.unwrap()
		.version_id
		.unwrap()
}

#[tokio::test]
async fn test_object_lock_configuration() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("object-lock-config");

	// Object Lock is off by default
	assert!(ctx
		.client
		.get_object_lock_configuration()
		.bucket(&bucket)
		.send()
		.await
		.is_err());

	// It cannot be turned on for a bucket that does not keep versions
	assert!(ctx
		.client
		.put_object_lock_configuration()
		.bucket(&bucket)
		.object_lock_configuration(
			ObjectLockConfiguration::builder()
				.object_lock_enabled(ObjectLockEnabled::Enabled)
				.build(),
		)
		.send()
		.await
		.is_err());

	let bucket = locked_bucket(
		&ctx,
		"object-lock-config-on",
		Some(
			DefaultRetention::builder()
				.mode(ObjectLockRetentionMode::Compliance)
				.days(7)
				.build(),
		),
	)
	.await;

	let r = ctx
		.client
		.get_object_lock_configuration()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let conf = r.object_lock_configuration.unwrap();
	assert_eq!(conf.object_lock_enabled, Some(ObjectLockEnabled::Enabled));
	let dr = conf.rule.unwrap().default_retention.unwrap();
	assert_eq!(dr.mode, Some(ObjectLockRetentionMode::Compliance));
	assert_eq!(dr.days, Some(7));

	// Versioning cannot be suspended while Object Lock is on, as that would
	// stop keeping the versions it protects
	assert!(ctx
		.client
		.put_bucket_versioning()
		.bucket(&bucket)
		.versioning_configuration(
			VersioningConfiguration::builder()
				.status(BucketVersioningStatus::Suspended)
				.build(),
		)
		.send()
		.await
		.is_err());
}

#[tokio::test]
async fn test_governance_retention() {
	let ctx = common::context();
	let bucket = locked_bucket(&ctx, "object-lock-governance", None).await;
	let version_id = put_object(&ctx, &bucket).await;

	let retain_until = in_the_future(3600);
	ctx.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.retention(
			ObjectLockRetention::builder()
				.mode(ObjectLockRetentionMode::Governance)
				.retain_until_date(retain_until)
				.build(),
		)
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	let retention = r.retention.unwrap();
	assert_eq!(retention.mode, Some(ObjectLockRetentionMode::Governance));
	assert_eq!(
		retention.retain_until_date.unwrap().to_millis().unwrap(),
		retain_until.to_millis().unwrap()
	);

	// GetObject reports the lock settings of the version
	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();
	assert_eq!(o.object_lock_mode, Some(ObjectLockMode::Governance));

	// The version cannot be deleted while it is retained ...
	assert!(ctx
		.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.is_err());

	// ... nor can its retention be shortened ...
	assert!(ctx
		.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.retention(
			ObjectLockRetention::builder()
				.mode(ObjectLockRetentionMode::Governance)
				.retain_until_date(in_the_future(60))
				.build(),
		)
		.send()
		.await
		.is_err());

	// ... but a delete marker can still be created, as in AWS S3
	ctx.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.unwrap();

	// A caller that bypasses governance retention can shorten it and delete
	// the version
	ctx.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.bypass_governance_retention(true)
		.retention(ObjectLockRetention::builder().build())
		.send()
		.await
		.unwrap();

	ctx.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
}

#[tokio::test]
async fn test_compliance_retention() {
	let ctx = common::context();
	let bucket = locked_bucket(&ctx, "object-lock-compliance", None).await;
	let version_id = put_object(&ctx, &bucket).await;

	ctx.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.retention(
			ObjectLockRetention::builder()
				.mode(ObjectLockRetentionMode::Compliance)
				.retain_until_date(in_the_future(3600))
				.build(),
		)
		.send()
		.await
		.unwrap();

	// Not even a caller that bypasses governance retention can delete the
	// version or relax its retention
	assert!(ctx
		.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.bypass_governance_retention(true)
		.send()
		.await
		.is_err());

	assert!(ctx
		.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.bypass_governance_retention(true)
		.retention(ObjectLockRetention::builder().build())
		.send()
		.await
		.is_err());

	assert!(ctx
		.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.bypass_governance_retention(true)
		.retention(
			ObjectLockRetention::builder()
				.mode(ObjectLockRetentionMode::Governance)
				.retain_until_date(in_the_future(7200))
				.build(),
		)
		.send()
		.await
		.is_err());

	// The retention can only be extended
	let later = in_the_future(7200);
	ctx.client
		.put_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.retention(
			ObjectLockRetention::builder()
				.mode(ObjectLockRetentionMode::Compliance)
				.retain_until_date(later)
				.build(),
		)
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_eq!(
		r.retention
			.unwrap()
			.retain_until_date
			.unwrap()
			.to_millis()
			.unwrap(),
		later.to_millis().unwrap()
	);

	// The data is still readable
	let o = ctx
		.client
		.get_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_bytes_eq!(o.body, BODY);

	// `garage bucket inspect-object` reports the S3 version id of the version
	// and the retention that protects it
	let output = ctx
		.garage
		.command()
		.args(["bucket", "inspect-object", &bucket, KEY])
		.expect_success_output("Could not inspect object");
	let stdout = String::from_utf8(output.stdout).unwrap();
	assert!(stdout.contains(&version_id), "{}", stdout);
	assert!(stdout.contains("compliance mode"), "{}", stdout);

	// And the bucket cannot be deleted while it holds retained versions
	let output = ctx
		.garage
		.command()
		.args(["bucket", "delete", "--yes", &bucket])
		.output()
		.expect("Could not run garage bucket delete");
	assert!(!output.status.success());
}

#[tokio::test]
async fn test_legal_hold() {
	let ctx = common::context();
	let bucket = locked_bucket(&ctx, "object-lock-legal-hold", None).await;
	let version_id = put_object(&ctx, &bucket).await;

	// No legal hold to start with
	let r = ctx
		.client
		.get_object_legal_hold()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_eq!(
		r.legal_hold.unwrap().status,
		Some(ObjectLockLegalHoldStatus::Off)
	);

	ctx.client
		.put_object_legal_hold()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.legal_hold(
			ObjectLockLegalHold::builder()
				.status(ObjectLockLegalHoldStatus::On)
				.build(),
		)
		.send()
		.await
		.unwrap();

	let r = ctx
		.client
		.get_object_legal_hold()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_eq!(
		r.legal_hold.unwrap().status,
		Some(ObjectLockLegalHoldStatus::On)
	);

	// A legal hold cannot be bypassed
	assert!(ctx
		.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.bypass_governance_retention(true)
		.send()
		.await
		.is_err());

	// Once it is lifted, the version can be deleted again
	ctx.client
		.put_object_legal_hold()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.legal_hold(
			ObjectLockLegalHold::builder()
				.status(ObjectLockLegalHoldStatus::Off)
				.build(),
		)
		.send()
		.await
		.unwrap();

	ctx.client
		.delete_object()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
}

#[tokio::test]
async fn test_object_lock_headers_and_default_retention() {
	let ctx = common::context();
	let bucket = locked_bucket(
		&ctx,
		"object-lock-defaults",
		Some(
			DefaultRetention::builder()
				.mode(ObjectLockRetentionMode::Governance)
				.days(1)
				.build(),
		),
	)
	.await;

	// An object written without any Object Lock header gets the retention that
	// the bucket applies by default
	let version_id = put_object(&ctx, &bucket).await;
	let r = ctx
		.client
		.get_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_eq!(
		r.retention.unwrap().mode,
		Some(ObjectLockRetentionMode::Governance)
	);

	// The headers of the request take precedence over the bucket default
	let retain_until = in_the_future(7200);
	let r = ctx
		.client
		.put_object()
		.bucket(&bucket)
		.key("explicit-lock")
		.body(ByteStream::from_static(BODY))
		.object_lock_mode(ObjectLockMode::Compliance)
		.object_lock_retain_until_date(retain_until)
		.object_lock_legal_hold_status(ObjectLockLegalHoldStatus::On)
		.send()
		.await
		.unwrap();
	let version_id = r.version_id.unwrap();

	let r = ctx
		.client
		.get_object_retention()
		.bucket(&bucket)
		.key("explicit-lock")
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	let retention = r.retention.unwrap();
	assert_eq!(retention.mode, Some(ObjectLockRetentionMode::Compliance));
	assert_eq!(
		retention.retain_until_date.unwrap().to_millis().unwrap(),
		retain_until.to_millis().unwrap()
	);

	let r = ctx
		.client
		.get_object_legal_hold()
		.bucket(&bucket)
		.key("explicit-lock")
		.version_id(&version_id)
		.send()
		.await
		.unwrap();
	assert_eq!(
		r.legal_hold.unwrap().status,
		Some(ObjectLockLegalHoldStatus::On)
	);
}

#[tokio::test]
async fn test_object_lock_requires_a_locked_bucket() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("object-lock-absent");

	// Writing an object with Object Lock headers to a bucket that does not have
	// Object Lock enabled is an error
	assert!(ctx
		.client
		.put_object()
		.bucket(&bucket)
		.key(KEY)
		.body(ByteStream::from_static(BODY))
		.object_lock_mode(ObjectLockMode::Governance)
		.object_lock_retain_until_date(in_the_future(3600))
		.send()
		.await
		.is_err());

	put_object(&ctx, &bucket).await;

	// So is asking for the retention of one of its objects
	assert!(ctx
		.client
		.get_object_retention()
		.bucket(&bucket)
		.key(KEY)
		.send()
		.await
		.is_err());
}

#[tokio::test]
async fn test_object_lock_from_the_cli() {
	let ctx = common::context();
	let bucket = ctx.create_bucket("object-lock-cli");

	ctx.garage
		.command()
		.args(["bucket", "object-lock", "--enable"])
		.args(["--mode", "compliance", "--years", "1"])
		.arg(&bucket)
		.quiet()
		.expect_success_status("Could not enable Object Lock on bucket");

	let r = ctx
		.client
		.get_object_lock_configuration()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	let conf = r.object_lock_configuration.unwrap();
	assert_eq!(conf.object_lock_enabled, Some(ObjectLockEnabled::Enabled));
	let dr = conf.rule.unwrap().default_retention.unwrap();
	assert_eq!(dr.mode, Some(ObjectLockRetentionMode::Compliance));
	assert_eq!(dr.years, Some(1));

	// Enabling Object Lock also turns versioning on
	let r = ctx
		.client
		.get_bucket_versioning()
		.bucket(&bucket)
		.send()
		.await
		.unwrap();
	assert_eq!(r.status, Some(BucketVersioningStatus::Enabled));
}
