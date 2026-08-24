//! Object Lock: bucket configuration, per-version retention and legal holds
//!
//! Object Lock can only be turned on for a bucket that has versioning enabled,
//! and can never be turned off afterwards. It protects individual versions of
//! objects, either through a retention period or through a legal hold:
//!
//! - a version under a legal hold cannot be deleted by anyone until the hold is
//!   lifted;
//! - a version retained in governance mode can only be deleted by a caller that
//!   asks to bypass governance retention and has the `owner` permission on the
//!   bucket;
//! - a version retained in compliance mode cannot be deleted by anyone, not
//!   even a cluster administrator, before its retain-until date.

use std::convert::TryFrom;

use chrono::{DateTime, Utc};
use quick_xml::de::from_reader;
use serde::{Deserialize, Serialize};

use http::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::{Request, Response, StatusCode};

use garage_util::time::{msec_to_rfc3339, now_msec};

use garage_model::bucket_table::{
	Bucket, BucketParams, DefaultRetention, ObjectLockConfig, RetentionDuration, VersioningState,
};
use garage_model::s3::object_table::{
	DeleteProtection, Object, ObjectLockMode, ObjectVersion, Retention,
};

use garage_api_common::helpers::*;

use crate::api_server::{ReqBody, ResBody};
use crate::error::*;
use crate::versioning::requested_version_id;
use crate::xml::{to_xml_with_header, xmlns_tag};

pub(crate) const X_AMZ_BUCKET_OBJECT_LOCK_ENABLED: HeaderName =
	HeaderName::from_static("x-amz-bucket-object-lock-enabled");
pub(crate) const X_AMZ_OBJECT_LOCK_MODE: HeaderName =
	HeaderName::from_static("x-amz-object-lock-mode");
pub(crate) const X_AMZ_OBJECT_LOCK_RETAIN_UNTIL_DATE: HeaderName =
	HeaderName::from_static("x-amz-object-lock-retain-until-date");
pub(crate) const X_AMZ_OBJECT_LOCK_LEGAL_HOLD: HeaderName =
	HeaderName::from_static("x-amz-object-lock-legal-hold");
pub(crate) const X_AMZ_BYPASS_GOVERNANCE_RETENTION: HeaderName =
	HeaderName::from_static("x-amz-bypass-governance-retention");

const GOVERNANCE: &str = "GOVERNANCE";
const COMPLIANCE: &str = "COMPLIANCE";
const ON: &str = "ON";
const OFF: &str = "OFF";
const ENABLED: &str = "Enabled";

// ---- XML documents of the Object Lock endpoints ----

mod lock_xml {
	use super::*;

	#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
	#[serde(rename = "ObjectLockConfiguration")]
	pub struct ObjectLockConfiguration {
		#[serde(rename = "@xmlns", serialize_with = "xmlns_tag", skip_deserializing)]
		pub xmlns: (),
		#[serde(
			rename = "ObjectLockEnabled",
			default,
			skip_serializing_if = "Option::is_none"
		)]
		pub object_lock_enabled: Option<String>,
		#[serde(rename = "Rule", default, skip_serializing_if = "Option::is_none")]
		pub rule: Option<Rule>,
	}

	#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
	pub struct Rule {
		#[serde(
			rename = "DefaultRetention",
			default,
			skip_serializing_if = "Option::is_none"
		)]
		pub default_retention: Option<DefaultRetention>,
	}

	#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
	pub struct DefaultRetention {
		#[serde(rename = "Mode", default, skip_serializing_if = "Option::is_none")]
		pub mode: Option<String>,
		#[serde(rename = "Days", default, skip_serializing_if = "Option::is_none")]
		pub days: Option<u64>,
		#[serde(rename = "Years", default, skip_serializing_if = "Option::is_none")]
		pub years: Option<u64>,
	}

	#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
	#[serde(rename = "Retention")]
	pub struct Retention {
		#[serde(rename = "@xmlns", serialize_with = "xmlns_tag", skip_deserializing)]
		pub xmlns: (),
		#[serde(rename = "Mode", default, skip_serializing_if = "Option::is_none")]
		pub mode: Option<String>,
		#[serde(
			rename = "RetainUntilDate",
			default,
			skip_serializing_if = "Option::is_none"
		)]
		pub retain_until_date: Option<String>,
	}

	#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
	#[serde(rename = "LegalHold")]
	pub struct LegalHold {
		#[serde(rename = "@xmlns", serialize_with = "xmlns_tag", skip_deserializing)]
		pub xmlns: (),
		#[serde(rename = "Status")]
		pub status: String,
	}
}

// ---- Conversions between the wire format and the stored configuration ----

fn parse_mode(mode: &str) -> Result<ObjectLockMode, Error> {
	match mode {
		GOVERNANCE => Ok(ObjectLockMode::Governance),
		COMPLIANCE => Ok(ObjectLockMode::Compliance),
		_ => Err(Error::bad_request(format!(
			"Invalid Object Lock mode `{}`, expected {} or {}",
			mode, GOVERNANCE, COMPLIANCE
		))),
	}
}

fn mode_str(mode: ObjectLockMode) -> &'static str {
	match mode {
		ObjectLockMode::Governance => GOVERNANCE,
		ObjectLockMode::Compliance => COMPLIANCE,
	}
}

fn parse_retain_until_date(date: &str) -> Result<u64, Error> {
	let date = DateTime::parse_from_rfc3339(date).map_err(|_| {
		Error::bad_request(format!(
			"Invalid retain-until date `{}`, expected an ISO 8601 date",
			date
		))
	})?;
	u64::try_from(date.with_timezone(&Utc).timestamp_millis())
		.map_err(|_| Error::bad_request("Retain-until date is out of range"))
}

impl lock_xml::DefaultRetention {
	fn validate(&self) -> Result<DefaultRetention, Error> {
		let mode = parse_mode(
			self.mode
				.as_deref()
				.ok_or_bad_request("DefaultRetention must specify a Mode")?,
		)?;
		let duration = match (self.days, self.years) {
			(Some(_), Some(_)) => {
				return Err(Error::bad_request(
					"DefaultRetention cannot specify both Days and Years",
				))
			}
			(Some(days), None) => RetentionDuration::Days(days),
			(None, Some(years)) => RetentionDuration::Years(years),
			(None, None) => {
				return Err(Error::bad_request(
					"DefaultRetention must specify either Days or Years",
				))
			}
		};
		match duration {
			RetentionDuration::Days(0) | RetentionDuration::Years(0) => {
				return Err(Error::bad_request(
					"DefaultRetention period must be at least one day",
				))
			}
			_ => (),
		}
		Ok(DefaultRetention { mode, duration })
	}

	fn from_garage(retention: &DefaultRetention) -> Self {
		let (days, years) = match retention.duration {
			RetentionDuration::Days(d) => (Some(d), None),
			RetentionDuration::Years(y) => (None, Some(y)),
		};
		Self {
			mode: Some(mode_str(retention.mode).to_string()),
			days,
			years,
		}
	}
}

// ---- GetObjectLockConfiguration / PutObjectLockConfiguration ----

pub async fn handle_get_object_lock_configuration(ctx: ReqCtx) -> Result<Response<ResBody>, Error> {
	let ReqCtx { bucket_params, .. } = ctx;

	let config = bucket_params
		.object_lock()
		.ok_or(Error::ObjectLockConfigurationNotFound)?;

	let conf = lock_xml::ObjectLockConfiguration {
		xmlns: (),
		object_lock_enabled: Some(ENABLED.to_string()),
		rule: config.default_retention.as_ref().map(|r| lock_xml::Rule {
			default_retention: Some(lock_xml::DefaultRetention::from_garage(r)),
		}),
	};

	let xml = to_xml_with_header(&conf)?;
	Ok(Response::builder()
		.header("Content-Type", "application/xml")
		.body(string_body(xml))?)
}

pub async fn handle_put_object_lock_configuration(
	ctx: ReqCtx,
	req: Request<ReqBody>,
) -> Result<Response<ResBody>, Error> {
	let ReqCtx {
		garage,
		bucket_id,
		mut bucket_params,
		..
	} = ctx;

	let body = req.into_body().collect().await?;
	let conf: lock_xml::ObjectLockConfiguration = from_reader(&body as &[u8])?;

	// There is no way to turn Object Lock off: the request must either say it
	// is enabled, or say nothing about it and only change the default retention
	// of a bucket that already has it enabled.
	match conf.object_lock_enabled.as_deref() {
		Some(ENABLED) => (),
		None if bucket_params.object_lock().is_some() => (),
		Some(other) => {
			return Err(Error::bad_request(format!(
				"Invalid ObjectLockEnabled value `{}`, expected {}",
				other, ENABLED
			)))
		}
		None => {
			return Err(Error::bad_request(
				"ObjectLockEnabled must be set to Enabled to turn Object Lock on",
			))
		}
	}

	// Object Lock protects versions of objects, so it can only be turned on for
	// a bucket that keeps them.
	if bucket_params.versioning() != VersioningState::Enabled {
		return Err(Error::InvalidBucketState(
			"Object Lock requires bucket versioning to be enabled".into(),
		));
	}

	let default_retention = match &conf.rule {
		Some(rule) => match &rule.default_retention {
			Some(dr) => Some(dr.validate()?),
			None => None,
		},
		None => None,
	};

	bucket_params
		.object_lock
		.update(Some(ObjectLockConfig { default_retention }).into());
	garage
		.bucket_table
		.insert(&Bucket::present(bucket_id, bucket_params))
		.await?;

	Ok(Response::builder()
		.status(StatusCode::OK)
		.body(empty_body())?)
}

// ---- Per-version retention and legal hold ----

/// Fetch the version of an object that an Object Lock request refers to
async fn get_version(
	ctx: &ReqCtx,
	key: &str,
	version_id: Option<&str>,
) -> Result<ObjectVersion, Error> {
	let ReqCtx {
		garage,
		bucket_id,
		bucket_params,
		..
	} = ctx;

	if bucket_params.object_lock().is_none() {
		return Err(Error::bad_request(
			"Bucket is missing Object Lock Configuration",
		));
	}

	let object = garage
		.object_table
		.get(bucket_id, &key.to_string())
		.await?
		.ok_or(Error::NoSuchKey)?;

	let version = match requested_version_id(bucket_params, version_id) {
		Some(version_id) => object
			.version_by_id(version_id)
			.ok_or(Error::NoSuchVersion)?,
		None => object.current_version().ok_or(Error::NoSuchKey)?,
	};

	// A delete marker holds no data, so it cannot be locked
	if version.is_delete_marker() {
		return Err(Error::MethodNotAllowed);
	}

	Ok(version.clone())
}

async fn put_version(ctx: &ReqCtx, key: &str, version: ObjectVersion) -> Result<(), Error> {
	let object = Object::new(ctx.bucket_id, key.to_string(), vec![version]);
	ctx.garage.object_table.insert(&object).await?;
	Ok(())
}

pub async fn handle_get_object_retention(
	ctx: ReqCtx,
	key: &str,
	version_id: Option<&str>,
) -> Result<Response<ResBody>, Error> {
	let version = get_version(&ctx, key, version_id).await?;

	let retention = version
		.retention
		.get()
		.ok_or(Error::NoSuchObjectLockConfiguration)?;

	let xml = to_xml_with_header(&lock_xml::Retention {
		xmlns: (),
		mode: Some(mode_str(retention.mode).to_string()),
		retain_until_date: Some(msec_to_rfc3339(retention.retain_until)),
	})?;

	Ok(Response::builder()
		.header("Content-Type", "application/xml")
		.body(string_body(xml))?)
}

pub async fn handle_put_object_retention(
	ctx: ReqCtx,
	req: Request<ReqBody>,
	key: &str,
	version_id: Option<&str>,
) -> Result<Response<ResBody>, Error> {
	let bypass_governance = bypass_governance_retention(req.headers());

	let body = req.into_body().collect().await?;
	let conf: lock_xml::Retention = from_reader(&body as &[u8])?;

	let now = now_msec();
	let new_retention = match (&conf.mode, &conf.retain_until_date) {
		// An empty Retention document lifts the retention of the version
		(None, None) => None,
		(Some(mode), Some(date)) => {
			let retain_until = parse_retain_until_date(date)?;
			if retain_until <= now {
				return Err(Error::bad_request(
					"The retain-until date must be in the future",
				));
			}
			Some(Retention {
				mode: parse_mode(mode)?,
				retain_until,
			})
		}
		_ => {
			return Err(Error::bad_request(
				"Retention must specify both Mode and RetainUntilDate, or neither",
			))
		}
	};

	let mut version = get_version(&ctx, key, version_id).await?;
	check_retention_change(&ctx, &version, new_retention, bypass_governance, now)?;

	version.retention.update(new_retention);
	put_version(&ctx, key, version).await?;

	Ok(Response::builder()
		.status(StatusCode::OK)
		.body(empty_body())?)
}

/// Check that a change of the retention setting of a version is allowed
///
/// Making the retention stricter is always allowed. Relaxing it is only allowed
/// while the version is not retained any more, or, for governance mode, by a
/// caller that bypasses governance retention. A version retained in compliance
/// mode can never have its retention relaxed.
fn check_retention_change(
	ctx: &ReqCtx,
	version: &ObjectVersion,
	new: Option<Retention>,
	bypass_governance: bool,
	now: u64,
) -> Result<(), Error> {
	let old = match version.retention.get() {
		Some(old) if old.is_active_at(now) => *old,
		// The version is not currently retained, so any new setting is fine
		_ => return Ok(()),
	};

	let relaxing = match new {
		None => true,
		Some(new) => {
			new.retain_until < old.retain_until
				|| (old.mode == ObjectLockMode::Compliance
					&& new.mode == ObjectLockMode::Governance)
		}
	};
	if !relaxing {
		return Ok(());
	}

	match old.mode {
		ObjectLockMode::Compliance => Err(Error::forbidden(format!(
			"This version is retained in compliance mode until {}: \
			 its retention can neither be shortened nor lifted",
			msec_to_rfc3339(old.retain_until)
		))),
		ObjectLockMode::Governance => {
			if bypass_governance && ctx.api_key.allow_owner(&ctx.bucket_id) {
				Ok(())
			} else {
				Err(Error::forbidden(format!(
					"This version is retained in governance mode until {}: \
					 shortening or lifting its retention requires the \
					 {} header and the owner permission on the bucket",
					msec_to_rfc3339(old.retain_until),
					X_AMZ_BYPASS_GOVERNANCE_RETENTION
				)))
			}
		}
	}
}

pub async fn handle_get_object_legal_hold(
	ctx: ReqCtx,
	key: &str,
	version_id: Option<&str>,
) -> Result<Response<ResBody>, Error> {
	let version = get_version(&ctx, key, version_id).await?;

	let xml = to_xml_with_header(&lock_xml::LegalHold {
		xmlns: (),
		status: legal_hold_str(*version.legal_hold.get()).to_string(),
	})?;

	Ok(Response::builder()
		.header("Content-Type", "application/xml")
		.body(string_body(xml))?)
}

pub async fn handle_put_object_legal_hold(
	ctx: ReqCtx,
	req: Request<ReqBody>,
	key: &str,
	version_id: Option<&str>,
) -> Result<Response<ResBody>, Error> {
	let body = req.into_body().collect().await?;
	let conf: lock_xml::LegalHold = from_reader(&body as &[u8])?;
	let legal_hold = parse_legal_hold(&conf.status)?;

	let mut version = get_version(&ctx, key, version_id).await?;
	if *version.legal_hold.get() != legal_hold {
		version.legal_hold.update(legal_hold);
		put_version(&ctx, key, version).await?;
	}

	Ok(Response::builder()
		.status(StatusCode::OK)
		.body(empty_body())?)
}

fn parse_legal_hold(status: &str) -> Result<bool, Error> {
	match status {
		ON => Ok(true),
		OFF => Ok(false),
		_ => Err(Error::bad_request(format!(
			"Invalid legal hold status `{}`, expected {} or {}",
			status, ON, OFF
		))),
	}
}

fn legal_hold_str(legal_hold: bool) -> &'static str {
	match legal_hold {
		true => ON,
		false => OFF,
	}
}

// ---- Object Lock settings of newly created versions ----

/// The Object Lock settings that a request asks to apply to the version it
/// creates, or that the bucket applies by default
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ObjectLockSettings {
	pub(crate) retention: Option<Retention>,
	pub(crate) legal_hold: bool,
}

impl ObjectLockSettings {
	/// Apply these settings to a version that is being created
	pub(crate) fn apply(&self, version: &mut ObjectVersion) {
		if self.retention.is_some() {
			version.retention.update(self.retention);
		}
		if self.legal_hold {
			version.legal_hold.update(true);
		}
	}
}

/// Read the Object Lock settings that apply to a version being created, from
/// the `x-amz-object-lock-*` headers of the request and, for whatever they do
/// not specify, from the default retention of the bucket
pub(crate) fn object_lock_from_headers(
	bucket_params: &BucketParams,
	headers: &HeaderMap<HeaderValue>,
) -> Result<ObjectLockSettings, Error> {
	let mode = get_header(headers, &X_AMZ_OBJECT_LOCK_MODE)?;
	let retain_until = get_header(headers, &X_AMZ_OBJECT_LOCK_RETAIN_UNTIL_DATE)?;
	let legal_hold = get_header(headers, &X_AMZ_OBJECT_LOCK_LEGAL_HOLD)?;

	let config = match bucket_params.object_lock() {
		Some(config) => config,
		None => {
			if mode.is_some() || retain_until.is_some() || legal_hold.is_some() {
				return Err(Error::bad_request(
					"Bucket is missing Object Lock Configuration",
				));
			}
			return Ok(ObjectLockSettings::default());
		}
	};

	let retention = match (mode, retain_until) {
		(Some(mode), Some(date)) => {
			let retain_until = parse_retain_until_date(&date)?;
			if retain_until <= now_msec() {
				return Err(Error::bad_request(
					"The retain-until date must be in the future",
				));
			}
			Some(Retention {
				mode: parse_mode(&mode)?,
				retain_until,
			})
		}
		(None, None) => config.default_retention.map(|dr| Retention {
			mode: dr.mode,
			retain_until: now_msec().saturating_add(dr.duration.as_msec()),
		}),
		_ => {
			return Err(Error::bad_request(format!(
				"{} and {} must be specified together",
				X_AMZ_OBJECT_LOCK_MODE, X_AMZ_OBJECT_LOCK_RETAIN_UNTIL_DATE
			)))
		}
	};

	Ok(ObjectLockSettings {
		retention,
		legal_hold: legal_hold
			.map(|s| parse_legal_hold(&s))
			.transpose()?
			.unwrap_or(false),
	})
}

fn get_header(
	headers: &HeaderMap<HeaderValue>,
	name: &HeaderName,
) -> Result<Option<String>, Error> {
	match headers.get(name) {
		Some(v) => Ok(Some(v.to_str()?.to_string())),
		None => Ok(None),
	}
}

/// Whether the bucket the request is addressed to was asked to be created with
/// Object Lock enabled
pub(crate) fn create_bucket_object_lock_enabled(
	headers: &HeaderMap<HeaderValue>,
) -> Result<bool, Error> {
	match get_header(headers, &X_AMZ_BUCKET_OBJECT_LOCK_ENABLED)? {
		None => Ok(false),
		Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
		Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
		Some(v) => Err(Error::bad_request(format!(
			"Invalid {} value `{}`, expected true or false",
			X_AMZ_BUCKET_OBJECT_LOCK_ENABLED, v
		))),
	}
}

// ---- Enforcement ----

/// Whether the request asks to bypass governance-mode retention
pub(crate) fn bypass_governance_retention(headers: &HeaderMap<HeaderValue>) -> bool {
	headers
		.get(&X_AMZ_BYPASS_GOVERNANCE_RETENTION)
		.and_then(|v| v.to_str().ok())
		.map(|v| v.eq_ignore_ascii_case("true"))
		.unwrap_or(false)
}

/// Check that Object Lock allows a version to be permanently deleted
pub(crate) fn check_delete_allowed(
	ctx: &ReqCtx,
	version: &ObjectVersion,
	bypass_governance: bool,
) -> Result<(), Error> {
	match version.delete_protection(now_msec()) {
		DeleteProtection::None => Ok(()),
		DeleteProtection::LegalHold => Err(Error::forbidden(
			"This version is under a legal hold and cannot be deleted until it is lifted",
		)),
		DeleteProtection::Compliance { retain_until } => Err(Error::forbidden(format!(
			"This version is retained in compliance mode until {} and cannot be deleted",
			msec_to_rfc3339(retain_until)
		))),
		DeleteProtection::Governance { retain_until } => {
			if bypass_governance && ctx.api_key.allow_owner(&ctx.bucket_id) {
				Ok(())
			} else {
				Err(Error::forbidden(format!(
					"This version is retained in governance mode until {}: \
					 deleting it requires the {} header and the owner \
					 permission on the bucket",
					msec_to_rfc3339(retain_until),
					X_AMZ_BYPASS_GOVERNANCE_RETENTION
				)))
			}
		}
	}
}

/// Add the `x-amz-object-lock-*` headers describing the lock settings of a
/// version to a response
pub(crate) fn add_object_lock_response_headers(
	version: &ObjectVersion,
	mut resp: http::response::Builder,
) -> http::response::Builder {
	if let Some(retention) = version.retention.get() {
		resp = resp
			.header(X_AMZ_OBJECT_LOCK_MODE, mode_str(retention.mode))
			.header(
				X_AMZ_OBJECT_LOCK_RETAIN_UNTIL_DATE,
				msec_to_rfc3339(retention.retain_until),
			);
	}
	if *version.legal_hold.get() {
		resp = resp.header(X_AMZ_OBJECT_LOCK_LEGAL_HOLD, ON);
	}
	resp
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse_config(body: &str) -> lock_xml::ObjectLockConfiguration {
		from_reader(body.as_bytes()).unwrap()
	}

	#[test]
	fn parse_object_lock_configuration() {
		let conf = parse_config(
			r#"<ObjectLockConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
				<ObjectLockEnabled>Enabled</ObjectLockEnabled>
				<Rule>
					<DefaultRetention>
						<Mode>COMPLIANCE</Mode>
						<Days>30</Days>
					</DefaultRetention>
				</Rule>
			</ObjectLockConfiguration>"#,
		);
		assert_eq!(conf.object_lock_enabled.as_deref(), Some("Enabled"));
		let dr = conf.rule.unwrap().default_retention.unwrap();
		let dr = dr.validate().unwrap();
		assert_eq!(dr.mode, ObjectLockMode::Compliance);
		assert_eq!(dr.duration, RetentionDuration::Days(30));
	}

	#[test]
	fn parse_object_lock_configuration_without_rule() {
		let conf = parse_config(
			r#"<ObjectLockConfiguration>
				<ObjectLockEnabled>Enabled</ObjectLockEnabled>
			</ObjectLockConfiguration>"#,
		);
		assert_eq!(conf.object_lock_enabled.as_deref(), Some("Enabled"));
		assert_eq!(conf.rule, None);
	}

	#[test]
	fn default_retention_requires_exactly_one_duration() {
		let both = lock_xml::DefaultRetention {
			mode: Some(GOVERNANCE.into()),
			days: Some(1),
			years: Some(1),
		};
		assert!(both.validate().is_err());

		let neither = lock_xml::DefaultRetention {
			mode: Some(GOVERNANCE.into()),
			days: None,
			years: None,
		};
		assert!(neither.validate().is_err());

		let zero = lock_xml::DefaultRetention {
			mode: Some(GOVERNANCE.into()),
			days: Some(0),
			years: None,
		};
		assert!(zero.validate().is_err());
	}

	#[test]
	fn parse_retention_document() {
		let r: lock_xml::Retention = from_reader(
			r#"<Retention>
				<Mode>GOVERNANCE</Mode>
				<RetainUntilDate>2030-01-01T00:00:00.000Z</RetainUntilDate>
			</Retention>"#
				.as_bytes(),
		)
		.unwrap();
		assert_eq!(r.mode.as_deref(), Some(GOVERNANCE));
		assert_eq!(
			parse_retain_until_date(r.retain_until_date.as_deref().unwrap()).unwrap(),
			1_893_456_000_000
		);
	}

	#[test]
	fn parse_empty_retention_document() {
		let r: lock_xml::Retention = from_reader(r#"<Retention></Retention>"#.as_bytes()).unwrap();
		assert_eq!(r.mode, None);
		assert_eq!(r.retain_until_date, None);
	}

	#[test]
	fn parse_legal_hold_document() {
		let h: lock_xml::LegalHold =
			from_reader(r#"<LegalHold><Status>ON</Status></LegalHold>"#.as_bytes()).unwrap();
		assert!(parse_legal_hold(&h.status).unwrap());
	}

	#[test]
	fn retain_until_dates() {
		assert!(parse_retain_until_date("2030-01-01T00:00:00Z").is_ok());
		assert!(parse_retain_until_date("not a date").is_err());
	}
}
