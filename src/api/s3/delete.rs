use hyper::{Request, Response, StatusCode};

use garage_util::data::*;

use garage_model::bucket_table::VersioningState;
use garage_model::s3::object_table::*;

use garage_api_common::helpers::*;

use crate::api_server::{ReqBody, ResBody};
use crate::error::*;
use crate::object_lock::{bypass_governance_retention, check_delete_allowed};
use crate::put::next_timestamp;
use crate::versioning::{requested_version_id, X_AMZ_DELETE_MARKER, X_AMZ_VERSION_ID};
use crate::xml as s3_xml;

/// What a delete request did to an object
#[derive(Default)]
struct DeleteOutcome {
	/// The version id to report back to the client
	version_id: Option<String>,
	/// Whether a delete marker was created, or the version that was
	/// permanently deleted was one
	delete_marker: bool,
	/// The version id of the delete marker, when there is one
	delete_marker_version_id: Option<String>,
}

async fn handle_delete_internal(
	ctx: &ReqCtx,
	key: &str,
	version_id: Option<&str>,
	bypass_governance: bool,
) -> Result<DeleteOutcome, Error> {
	let ReqCtx {
		garage,
		bucket_id,
		bucket_params,
		..
	} = ctx;
	let object = garage
		.object_table
		.get(bucket_id, &key.to_string())
		.await?
		.ok_or(Error::NoSuchKey)?; // No need to delete

	match requested_version_id(bucket_params, version_id) {
		// Permanently delete one specific version of the object. The version is
		// put in the Aborted state rather than dropped from the object, so that
		// the deletion is propagated to the other nodes and the data that the
		// version references is garbage-collected.
		Some(version_id) => {
			let version = object
				.version_by_id(version_id)
				.ok_or(Error::NoSuchVersion)?;
			check_delete_allowed(ctx, version, bypass_governance)?;
			let delete_marker = version.is_delete_marker();

			let deleted_version = ObjectVersion {
				state: ObjectVersionState::Aborted,
				..version.clone()
			};
			let object = Object::new(*bucket_id, key.into(), vec![deleted_version]);
			garage.object_table.insert(&object).await?;

			Ok(DeleteOutcome {
				version_id: Some(version_id.to_string()),
				delete_marker,
				delete_marker_version_id: match delete_marker {
					true => Some(version_id.to_string()),
					false => None,
				},
			})
		}
		// Hide the current version of the object behind a delete marker. On a
		// versioned bucket, the previous versions are kept and can still be
		// read by their version id.
		None => {
			let del_timestamp = next_timestamp(Some(&object));
			let marker = ObjectVersion::new(
				gen_uuid(),
				del_timestamp,
				ObjectVersionKind::for_versioning_state(bucket_params.versioning()),
				ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
			);
			let marker_version_id = marker.version_id();

			let object = Object::new(*bucket_id, key.into(), vec![marker]);
			garage.object_table.insert(&object).await?;

			Ok(DeleteOutcome {
				version_id: Some(marker_version_id.clone()),
				delete_marker: true,
				delete_marker_version_id: Some(marker_version_id),
			})
		}
	}
}

pub async fn handle_delete(
	ctx: ReqCtx,
	key: &str,
	version_id: Option<&str>,
	bypass_governance: bool,
) -> Result<Response<ResBody>, Error> {
	let outcome = match handle_delete_internal(&ctx, key, version_id, bypass_governance).await {
		Ok(outcome) => outcome,
		// Deleting a key that does not exist is a success in S3
		Err(Error::NoSuchKey) => DeleteOutcome::default(),
		Err(e) => return Err(e),
	};

	let mut resp = Response::builder().status(StatusCode::NO_CONTENT);

	// As in AWS S3, buckets on which versioning was never enabled report
	// neither a version id nor a delete marker.
	if ctx.bucket_params.versioning() != VersioningState::Disabled {
		if outcome.delete_marker {
			resp = resp.header(X_AMZ_DELETE_MARKER, "true");
		}
		if let Some(version_id) = &outcome.version_id {
			resp = resp.header(X_AMZ_VERSION_ID, version_id);
		}
	}

	Ok(resp.body(empty_body())?)
}

pub async fn handle_delete_objects(
	ctx: ReqCtx,
	req: Request<ReqBody>,
) -> Result<Response<ResBody>, Error> {
	let bypass_governance = bypass_governance_retention(req.headers());

	let body = req.into_body().collect().await?;

	let cmd_xml = roxmltree::Document::parse(std::str::from_utf8(&body)?)?;
	let cmd = parse_delete_objects_xml(&cmd_xml).ok_or_bad_request("Invalid delete XML query")?;

	let mut ret_deleted = Vec::new();
	let mut ret_errors = Vec::new();

	// As in AWS S3, buckets on which versioning was never enabled report
	// neither version ids nor delete markers.
	let versioned = ctx.bucket_params.versioning() != VersioningState::Disabled;

	for obj in cmd.objects.iter() {
		match handle_delete_internal(&ctx, &obj.key, obj.version_id.as_deref(), bypass_governance)
			.await
		{
			Ok(outcome) => {
				if cmd.quiet {
					continue;
				}
				ret_deleted.push(s3_xml::Deleted {
					key: s3_xml::Value(obj.key.clone()),
					version_id: match versioned {
						true => outcome.version_id.map(s3_xml::Value),
						false => None,
					},
					delete_marker: match versioned && outcome.delete_marker {
						true => Some(s3_xml::Value("true".to_string())),
						false => None,
					},
					delete_marker_version_id: match versioned {
						true => outcome.delete_marker_version_id.map(s3_xml::Value),
						false => None,
					},
				});
			}
			Err(Error::NoSuchKey) => {
				if cmd.quiet {
					continue;
				}
				// Deleting a non-existent key is a success in S3
				ret_deleted.push(s3_xml::Deleted {
					key: s3_xml::Value(obj.key.clone()),
					version_id: None,
					delete_marker: None,
					delete_marker_version_id: None,
				});
			}
			Err(e) => {
				ret_errors.push(s3_xml::DeleteError {
					code: s3_xml::Value(e.aws_code().to_string()),
					key: Some(s3_xml::Value(obj.key.clone())),
					message: s3_xml::Value(format!("{}", e)),
					version_id: obj.version_id.clone().map(s3_xml::Value),
				});
			}
		}
	}

	let xml = s3_xml::to_xml_with_header(&s3_xml::DeleteResult {
		xmlns: (),
		deleted: ret_deleted,
		errors: ret_errors,
	})?;

	Ok(Response::builder()
		.header("Content-Type", "application/xml")
		.body(string_body(xml))?)
}

struct DeleteRequest {
	quiet: bool,
	objects: Vec<DeleteObject>,
}

struct DeleteObject {
	key: String,
	version_id: Option<String>,
}

fn parse_delete_objects_xml(xml: &roxmltree::Document) -> Option<DeleteRequest> {
	let mut ret = DeleteRequest {
		quiet: false,
		objects: vec![],
	};

	let root = xml.root();
	let delete = root.children().find(|n| n.is_element())?;

	if !delete.has_tag_name("Delete") {
		return None;
	}

	for item in delete.children() {
		// Skip text nodes introduced by formatted XML.
		if !item.is_element() {
			// text nodes are allowed only if they contain whitespace characters only
			if !item.text()?.trim().is_empty() {
				return None;
			}
			continue;
		}

		if item.has_tag_name("Object") {
			let key = item.children().find(|e| e.has_tag_name("Key"))?;
			let key_str = key.text()?;
			let version_id = item
				.children()
				.find(|e| e.has_tag_name("VersionId"))
				.and_then(|e| e.text())
				.map(str::to_string);
			ret.objects.push(DeleteObject {
				key: key_str.to_string(),
				version_id,
			});
		} else if item.has_tag_name("Quiet") {
			ret.quiet = item.text()? == "true";
		} else {
			return None;
		}
	}

	Some(ret)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_delete_objects_xml_with_formatting() {
		let body = r#"
			<Delete xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
			    <Object>
			        <Key>1_746573745f66696c65</Key>
			    </Object>
			    <Quiet>true</Quiet>
			</Delete>
		"#;
		let xml = roxmltree::Document::parse(body).expect("valid delete XML");
		let req = parse_delete_objects_xml(&xml).expect("request should be parsed");

		assert_eq!(req.objects.len(), 1);
		assert_eq!(req.objects[0].key, "1_746573745f66696c65");
		assert!(req.quiet);
	}

	#[test]
	fn parse_delete_objects_xml_rejects_non_whitespace_text_node() {
		let body = r#"<Delete xmlns="http://s3.amazonaws.com/doc/2006-03-01/">oops<Object><Key>1_746573745f66696c65</Key></Object></Delete>"#;
		let xml = roxmltree::Document::parse(body).expect("valid XML");
		let req = parse_delete_objects_xml(&xml);
		assert!(req.is_none());
	}

	#[test]
	fn parse_delete_objects_xml_rejects_pretty_print_with_stray_text() {
		let body = r#"
			<Delete xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
			    oops
			    <Object>
			        <Key>1_746573745f66696c65</Key>
			    </Object>
			</Delete>
		"#;
		let xml = roxmltree::Document::parse(body).expect("valid XML");
		let req = parse_delete_objects_xml(&xml);
		assert!(req.is_none());
	}

	#[test]
	fn parse_delete_objects_xml_accepts_compact_valid_xml() {
		let body = r#"<Delete xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Object><Key>1_746573745f66696c65</Key></Object><Quiet>false</Quiet></Delete>"#;
		let xml = roxmltree::Document::parse(body).expect("valid XML");
		let req = parse_delete_objects_xml(&xml).expect("request should be parsed");
		assert_eq!(req.objects.len(), 1);
		assert_eq!(req.objects[0].key, "1_746573745f66696c65");
		assert!(!req.quiet);
	}
}
