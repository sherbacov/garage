use quick_xml::de::from_reader;
use serde::Deserialize;

use http::header::HeaderName;
use hyper::{Request, Response, StatusCode};

use garage_util::data::Uuid;

use garage_model::bucket_table::{Bucket, BucketParams, VersioningState};
use garage_model::s3::object_table::NULL_VERSION_ID;

use garage_api_common::common_error::CommonError;
use garage_api_common::helpers::*;

use crate::api_server::{ReqBody, ResBody};
use crate::error::*;
use crate::xml as s3_xml;

pub(crate) const X_AMZ_VERSION_ID: HeaderName = HeaderName::from_static("x-amz-version-id");
pub(crate) const X_AMZ_DELETE_MARKER: HeaderName = HeaderName::from_static("x-amz-delete-marker");

/// The version id to report to the client for a version that was just created
/// in this bucket
pub(crate) fn response_version_id(bucket_params: &BucketParams, version_uuid: Uuid) -> String {
	match bucket_params.versioning() {
		VersioningState::Enabled => hex::encode(version_uuid),
		VersioningState::Suspended => NULL_VERSION_ID.to_string(),
		// On buckets on which versioning was never enabled, Garage keeps
		// reporting the internal version uuid, as it did before it supported
		// versioning
		VersioningState::Disabled => hex::encode(version_uuid),
	}
}

/// The version of an object that a request refers to, or None if it refers to
/// the object's current version
///
/// On a bucket on which versioning was never enabled, the `versionId`
/// parameter is ignored: Garage reports internal version uuids in
/// `x-amz-version-id` headers on such buckets, and clients that send them back
/// must keep addressing the object rather than get an error.
pub(crate) fn requested_version_id<'a>(
	bucket_params: &BucketParams,
	version_id: Option<&'a str>,
) -> Option<&'a str> {
	match bucket_params.versioning() {
		VersioningState::Disabled => None,
		VersioningState::Enabled | VersioningState::Suspended => version_id,
	}
}

/// The body of a `PutBucketVersioning` request
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename = "VersioningConfiguration")]
struct VersioningConfigurationRequest {
	#[serde(rename = "Status", default)]
	status: Option<String>,
	#[serde(rename = "MfaDelete", default)]
	mfa_delete: Option<String>,
}

pub fn handle_get_bucket_versioning(ctx: ReqCtx) -> Result<Response<ResBody>, Error> {
	let ReqCtx { bucket_params, .. } = ctx;

	// A bucket on which versioning was never enabled has no Status element at
	// all, which is different from a bucket whose versioning is suspended.
	let status = match bucket_params.versioning() {
		VersioningState::Disabled => None,
		VersioningState::Enabled => Some(s3_xml::Value("Enabled".to_string())),
		VersioningState::Suspended => Some(s3_xml::Value("Suspended".to_string())),
	};

	let versioning = s3_xml::VersioningConfiguration { xmlns: (), status };

	let xml = s3_xml::to_xml_with_header(&versioning)?;

	Ok(Response::builder()
		.header("Content-Type", "application/xml")
		.body(string_body(xml))?)
}

pub async fn handle_put_bucket_versioning(
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
	let conf: VersioningConfigurationRequest = from_reader(&body as &[u8])?;

	if let Some(mfa_delete) = &conf.mfa_delete {
		if !mfa_delete.eq_ignore_ascii_case("Disabled") {
			return Err(CommonError::NotImplemented(
				"MFA delete is not supported by Garage".into(),
			)
			.into());
		}
	}

	let new_state = match conf.status.as_deref() {
		Some("Enabled") => VersioningState::Enabled,
		Some("Suspended") => VersioningState::Suspended,
		_ => {
			return Err(Error::bad_request(
				"Invalid versioning status, expected Enabled or Suspended",
			))
		}
	};

	// Object Lock relies on versioning to keep the versions it protects, so
	// versioning cannot be suspended while Object Lock is enabled.
	if new_state == VersioningState::Suspended && bucket_params.object_lock().is_some() {
		return Err(Error::bad_request(
			"Versioning cannot be suspended on a bucket that has Object Lock enabled",
		));
	}

	if bucket_params.versioning() != new_state {
		bucket_params.versioning.update(new_state);
		garage
			.bucket_table
			.insert(&Bucket::present(bucket_id, bucket_params))
			.await?;
	}

	Ok(Response::builder()
		.status(StatusCode::OK)
		.body(empty_body())?)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(body: &str) -> Result<VersioningConfigurationRequest, quick_xml::de::DeError> {
		from_reader(body.as_bytes())
	}

	#[test]
	fn parse_enabled() {
		let conf = parse(
			r#"<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
				<Status>Enabled</Status>
			</VersioningConfiguration>"#,
		)
		.unwrap();
		assert_eq!(conf.status.as_deref(), Some("Enabled"));
		assert_eq!(conf.mfa_delete, None);
	}

	#[test]
	fn parse_suspended_with_mfa_delete() {
		let conf = parse(
			r#"<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
				<Status>Suspended</Status>
				<MfaDelete>Disabled</MfaDelete>
			</VersioningConfiguration>"#,
		)
		.unwrap();
		assert_eq!(conf.status.as_deref(), Some("Suspended"));
		assert_eq!(conf.mfa_delete.as_deref(), Some("Disabled"));
	}

	#[test]
	fn parse_empty() {
		let conf = parse(r#"<VersioningConfiguration></VersioningConfiguration>"#).unwrap();
		assert_eq!(conf.status, None);
	}
}
