use std::sync::Arc;

use async_trait::async_trait;
use chrono::prelude::*;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use garage_util::background::*;
use garage_util::data::*;
use garage_util::error::Error;
use garage_util::persister::PersisterShared;
use garage_util::time::*;

use garage_table::EmptyKey;

use crate::bucket_table::*;
use crate::s3::object_table::*;

use crate::garage::Garage;

mod v090 {
	use serde::{Deserialize, Serialize};

	#[derive(Serialize, Deserialize, Default, Clone)]
	pub struct LifecycleWorkerPersisted {
		pub last_completed: Option<String>,
	}

	impl garage_util::migrate::InitialFormat for LifecycleWorkerPersisted {
		const VERSION_MARKER: &'static [u8] = b"G09lwp";
	}
}

pub use v090::*;

pub struct LifecycleWorker {
	garage: Arc<Garage>,

	state: State,

	persister: PersisterShared<LifecycleWorkerPersisted>,
}

#[expect(clippy::large_enum_variant)]
enum State {
	Completed(NaiveDate),
	Running {
		date: NaiveDate,
		pos: Vec<u8>,
		counter: usize,
		objects_expired: usize,
		versions_expired: usize,
		mpu_aborted: usize,
		last_bucket: Option<Bucket>,
	},
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Skip {
	SkipBucket,
	NextObject,
}

pub fn register_bg_vars(
	persister: &PersisterShared<LifecycleWorkerPersisted>,
	vars: &mut vars::BgVars,
) {
	vars.register_ro(persister, "lifecycle-last-completed", |p| {
		p.get_with(|x| {
			x.last_completed
				.clone()
				.unwrap_or_else(|| "never".to_string())
		})
	});
}

impl LifecycleWorker {
	pub fn new(garage: Arc<Garage>, persister: PersisterShared<LifecycleWorkerPersisted>) -> Self {
		let today = today(garage.config.use_local_tz);
		let last_completed = persister.get_with(|x| {
			x.last_completed
				.as_deref()
				.and_then(|x| x.parse::<NaiveDate>().ok())
		});
		let state = match last_completed {
			Some(d) if d >= today => State::Completed(d),
			_ => State::start(today),
		};
		Self {
			garage,
			state,
			persister,
		}
	}
}

impl State {
	fn start(date: NaiveDate) -> Self {
		info!("Starting lifecycle worker for {}", date);
		State::Running {
			date,
			pos: vec![],
			counter: 0,
			objects_expired: 0,
			versions_expired: 0,
			mpu_aborted: 0,
			last_bucket: None,
		}
	}
}

#[async_trait]
impl Worker for LifecycleWorker {
	fn name(&self) -> String {
		"object lifecycle worker".to_string()
	}

	fn status(&self) -> WorkerStatus {
		match &self.state {
			State::Completed(d) => WorkerStatus {
				freeform: vec![format!("Last completed: {}", d)],
				..Default::default()
			},
			State::Running {
				date,
				counter,
				objects_expired,
				mpu_aborted,
				..
			} => {
				let n_objects = self.garage.object_table.data.store.approximate_len().ok();
				let progress = match n_objects {
					Some(total) if total > 0 => format!(
						"~{:.2}%",
						100. * std::cmp::min(*counter, total) as f32 / total as f32
					),
					_ => "...".to_string(),
				};
				WorkerStatus {
					progress: Some(progress),
					freeform: vec![
						format!("Started: {}", date),
						format!("Objects expired: {}", objects_expired),
						format!("Multipart uploads aborted: { }", mpu_aborted),
					],
					..Default::default()
				}
			}
		}
	}

	async fn work(&mut self, _must_exit: &mut watch::Receiver<bool>) -> Result<WorkerState, Error> {
		match &mut self.state {
			State::Completed(_) => Ok(WorkerState::Idle),
			State::Running {
				date,
				counter,
				objects_expired,
				versions_expired,
				mpu_aborted,
				pos,
				last_bucket,
			} => {
				// Process a batch of 100 items before yielding to bg task scheduler
				for _ in 0..100 {
					let (object_bytes, next_pos) = match self
						.garage
						.object_table
						.data
						.store
						.get_gt(&pos)?
					{
						None => {
							info!("Lifecycle worker finished for {}, objects expired: {}, noncurrent versions expired: {}, mpu aborted: {}", date, *objects_expired, *versions_expired, *mpu_aborted);
							self.persister
								.set_with(|x| x.last_completed = Some(date.to_string()))?;
							self.state = State::Completed(*date);
							return Ok(WorkerState::Idle);
						}
						Some((k, v)) => (v, k),
					};

					let object = self.garage.object_table.data.decode_entry(&object_bytes)?;
					let skip = process_object(
						&self.garage,
						*date,
						&object,
						objects_expired,
						versions_expired,
						mpu_aborted,
						last_bucket,
					)
					.await?;

					*counter += 1;
					if skip == Skip::SkipBucket {
						let bucket_id_len = object.bucket_id.as_slice().len();
						assert_eq!(
							next_pos.get(..bucket_id_len),
							Some(object.bucket_id.as_slice())
						);
						let last_bucket_pos = [&next_pos[..bucket_id_len], &[0xFFu8][..]].concat();
						*pos = std::cmp::max(next_pos, last_bucket_pos);
					} else {
						*pos = next_pos;
					}
				}

				Ok(WorkerState::Busy)
			}
		}
	}

	async fn wait_for_work(&mut self) -> WorkerState {
		match &self.state {
			State::Completed(d) => {
				let use_local_tz = self.garage.config.use_local_tz;
				let next_day = d.succ_opt().expect("no next day");
				let next_start = midnight_ts(next_day, use_local_tz);
				loop {
					let now = now_msec();
					if now < next_start {
						tokio::time::sleep_until(
							(Instant::now() + Duration::from_millis(next_start - now)).into(),
						)
						.await;
					} else {
						break;
					}
				}
				self.state = State::start(std::cmp::max(next_day, today(use_local_tz)));
			}
			State::Running { .. } => (),
		}
		WorkerState::Busy
	}
}

async fn process_object(
	garage: &Arc<Garage>,
	now_date: NaiveDate,
	object: &Object,
	objects_expired: &mut usize,
	versions_expired: &mut usize,
	mpu_aborted: &mut usize,
	last_bucket: &mut Option<Bucket>,
) -> Result<Skip, Error> {
	// An object that has more than one completely uploaded version has
	// noncurrent versions that a lifecycle rule may expire, even when none of
	// them holds data any more.
	let has_noncurrent_versions = object.versions().iter().filter(|v| v.is_complete()).count() > 1;
	if !has_noncurrent_versions
		&& !object
			.versions()
			.iter()
			.any(|x| x.is_data() || x.is_uploading(None))
	{
		return Ok(Skip::NextObject);
	}

	let bucket = match last_bucket.take() {
		Some(b) if b.id == object.bucket_id => b,
		_ => {
			match garage
				.bucket_table
				.get(&EmptyKey, &object.bucket_id)
				.await?
			{
				Some(b) => b,
				None => {
					warn!(
						"Lifecycle worker: object in non-existent bucket {:?}",
						object.bucket_id
					);
					return Ok(Skip::SkipBucket);
				}
			}
		}
	};

	let lifecycle_policy: &[LifecycleRule] = bucket
		.state
		.as_option()
		.and_then(|s| s.lifecycle_config.get().inner().map(|x| &x[..]))
		.unwrap_or_default();

	let versioning = bucket
		.params()
		.map(|s| s.versioning())
		.unwrap_or(VersioningState::Disabled);

	if lifecycle_policy.iter().all(|x| !x.enabled) {
		return Ok(Skip::SkipBucket);
	}

	let db = garage.object_table.data.store.db();

	for rule in lifecycle_policy.iter() {
		if !rule.enabled {
			continue;
		}

		if let Some(pfx) = &rule.filter.prefix {
			if !object.key.starts_with(pfx) {
				continue;
			}
		}

		if let Some(expire) = &rule.expiration {
			if let Some(current_version) = object.current_version().filter(|v| v.is_data()) {
				let version_date = next_date(current_version.timestamp);

				let current_version_data = match &current_version.state {
					ObjectVersionState::Complete(c) => c,
					_ => unreachable!(),
				};

				let size_match = check_size_filter(current_version_data, &rule.filter);
				let date_match = match expire {
					LifecycleExpiration::AfterDays(n_days) => {
						(now_date - version_date) >= chrono::Duration::days(*n_days as i64)
					}
					LifecycleExpiration::AtDate(exp_date) => {
						if let Ok(exp_date) = parse_lifecycle_date(exp_date) {
							now_date >= exp_date
						} else {
							warn!("Invalid expiration date stored in bucket {:?} lifecycle config: {}", bucket.id, exp_date);
							false
						}
					}
				};

				if size_match && date_match {
					// Delete expired version
					let deleted_object = Object::new(
						object.bucket_id,
						object.key.clone(),
						vec![ObjectVersion::new(
							gen_uuid(),
							std::cmp::max(now_msec(), current_version.timestamp + 1),
							ObjectVersionKind::for_versioning_state(versioning),
							ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
						)],
					);
					info!(
						"Lifecycle: expiring 1 object in bucket {:?}",
						object.bucket_id
					);
					db.transaction(|tx| garage.object_table.queue_insert(tx, &deleted_object))?;
					*objects_expired += 1;
				}
			}
		}

		if let Some(nv_exp) = &rule.noncurrent_version_expiration {
			let expired_versions =
				expired_noncurrent_versions(object, &rule.filter, nv_exp, now_date, now_msec())
					.into_iter()
					.map(|v| ObjectVersion {
						state: ObjectVersionState::Aborted,
						..v.clone()
					})
					.collect::<Vec<_>>();

			if !expired_versions.is_empty() {
				let n_expired = expired_versions.len();
				info!(
					"Lifecycle: expiring {} noncurrent version(s) in bucket {:?}",
					n_expired, object.bucket_id
				);
				let expired_object =
					Object::new(object.bucket_id, object.key.clone(), expired_versions);
				db.transaction(|tx| garage.object_table.queue_insert(tx, &expired_object))?;
				*versions_expired += n_expired;
			}
		}

		if let Some(abort_mpu_days) = &rule.abort_incomplete_mpu_days {
			let aborted_versions = object
				.versions()
				.iter()
				.filter_map(|v| {
					let version_date = next_date(v.timestamp);
					if (now_date - version_date) >= chrono::Duration::days(*abort_mpu_days as i64)
						&& matches!(&v.state, ObjectVersionState::Uploading { .. })
					{
						Some(ObjectVersion {
							state: ObjectVersionState::Aborted,
							..v.clone()
						})
					} else {
						None
					}
				})
				.collect::<Vec<_>>();
			if !aborted_versions.is_empty() {
				// Insert aborted mpu info
				let n_aborted = aborted_versions.len();
				info!(
					"Lifecycle: aborting {} incomplete upload(s) in bucket {:?}",
					n_aborted, object.bucket_id
				);
				let aborted_object =
					Object::new(object.bucket_id, object.key.clone(), aborted_versions);
				db.transaction(|tx| garage.object_table.queue_insert(tx, &aborted_object))?;
				*mpu_aborted += n_aborted;
			}
		}
	}

	*last_bucket = Some(bucket);
	Ok(Skip::NextObject)
}

#[expect(clippy::nonminimal_bool)]
fn check_size_filter(version_data: &ObjectVersionData, filter: &LifecycleFilter) -> bool {
	let size = match version_data {
		ObjectVersionData::Inline(meta, _) | ObjectVersionData::FirstBlock(meta, _) => meta.size,
		_ => unreachable!(),
	};
	if let Some(size_gt) = filter.size_gt {
		if !(size > size_gt) {
			return false;
		}
	}
	if let Some(size_lt) = filter.size_lt {
		if !(size < size_lt) {
			return false;
		}
	}
	true
}

/// The versions of an object that a `NoncurrentVersionExpiration` rule expires
///
/// The versions of an object are sorted from oldest to newest, and the last
/// completely uploaded one is its current version, which such a rule never
/// touches. Each of the others became noncurrent when the version that follows
/// it was written, which is the date the rule counts from.
fn expired_noncurrent_versions<'a>(
	object: &'a Object,
	filter: &LifecycleFilter,
	nv_exp: &NoncurrentVersionExpiration,
	now_date: NaiveDate,
	now_ms: u64,
) -> Vec<&'a ObjectVersion> {
	let complete = object
		.versions()
		.iter()
		.filter(|v| v.is_complete())
		.collect::<Vec<_>>();
	let n_noncurrent = complete.len().saturating_sub(1);

	let mut expired = Vec::new();
	for (i, version) in complete.iter().take(n_noncurrent).enumerate() {
		// Keep the most recent noncurrent versions if the rule says to
		if let Some(keep) = nv_exp.newer_noncurrent_versions {
			if n_noncurrent - 1 - i < keep {
				continue;
			}
		}

		if !check_version_size_filter(version, filter) {
			continue;
		}

		let noncurrent_date = next_date(complete[i + 1].timestamp);
		if (now_date - noncurrent_date) < chrono::Duration::days(nv_exp.noncurrent_days as i64) {
			continue;
		}

		// Object Lock protects a version even against the bucket's own
		// lifecycle rules
		if version.delete_protection(now_ms).is_protected() {
			continue;
		}

		expired.push(*version);
	}
	expired
}

/// Does a version match the size conditions of a lifecycle filter
///
/// A delete marker holds no data and so has no size: it only matches a filter
/// that has no size condition at all.
fn check_version_size_filter(version: &ObjectVersion, filter: &LifecycleFilter) -> bool {
	match &version.state {
		ObjectVersionState::Complete(data) => match data {
			ObjectVersionData::Inline(_, _) | ObjectVersionData::FirstBlock(_, _) => {
				check_size_filter(data, filter)
			}
			ObjectVersionData::DeleteMarker => filter.size_gt.is_none() && filter.size_lt.is_none(),
		},
		_ => false,
	}
}

fn midnight_ts(date: NaiveDate, use_local_tz: bool) -> u64 {
	let midnight = date.and_hms_opt(0, 0, 0).expect("midnight does not exist");
	if use_local_tz {
		return midnight
			.and_local_timezone(Local)
			.single()
			.expect("bad local midnight")
			.timestamp_millis() as u64;
	}
	midnight.and_utc().timestamp_millis() as u64
}

fn next_date(ts: u64) -> NaiveDate {
	DateTime::<Utc>::from_timestamp_millis(ts as i64)
		.expect("bad timestamp")
		.date_naive()
		.succ_opt()
		.expect("no next day")
}

fn today(use_local_tz: bool) -> NaiveDate {
	if use_local_tz {
		return Local::now().naive_local().date();
	}
	Utc::now().naive_utc().date()
}

#[cfg(test)]
mod tests {
	use super::*;

	use garage_util::data::Uuid;

	const DAY_MS: u64 = 24 * 3600 * 1000;

	fn uuid(n: u8) -> Uuid {
		Uuid::from([n; 32])
	}

	/// A date far enough from the epoch that we can place versions before it
	fn today() -> NaiveDate {
		NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()
	}

	fn now_ms() -> u64 {
		midnight_ts(today(), false)
	}

	/// A version created `days_ago` days before `today()`
	fn version(n: u8, days_ago: u64, size: u64) -> ObjectVersion {
		ObjectVersion::new(
			uuid(n),
			now_ms() - days_ago * DAY_MS,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Complete(ObjectVersionData::Inline(
				ObjectVersionMeta {
					size,
					etag: "d41d8cd98f00b204e9800998ecf8427e".into(),
					encryption: ObjectVersionEncryption::Plaintext {
						inner: ObjectVersionMetaInner {
							headers: vec![],
							checksum: None,
							checksum_type: None,
						},
					},
				},
				vec![],
			)),
		)
	}

	fn delete_marker(n: u8, days_ago: u64) -> ObjectVersion {
		ObjectVersion::new(
			uuid(n),
			now_ms() - days_ago * DAY_MS,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
		)
	}

	fn object(versions: Vec<ObjectVersion>) -> Object {
		Object::new(uuid(0xff), "the/key".into(), versions)
	}

	fn expire(
		object: &Object,
		nv_exp: &NoncurrentVersionExpiration,
		filter: &LifecycleFilter,
	) -> Vec<Uuid> {
		expired_noncurrent_versions(object, filter, nv_exp, today(), now_ms())
			.into_iter()
			.map(|v| v.uuid)
			.collect()
	}

	fn after(days: usize) -> NoncurrentVersionExpiration {
		NoncurrentVersionExpiration {
			noncurrent_days: days,
			newer_noncurrent_versions: None,
		}
	}

	#[test]
	fn the_current_version_never_expires() {
		// A single version is the current one, however old it is
		let obj = object(vec![version(1, 100, 10)]);
		assert!(expire(&obj, &after(1), &LifecycleFilter::default()).is_empty());
	}

	#[test]
	fn noncurrent_versions_expire_from_the_day_they_stopped_being_current() {
		// v1 became noncurrent when v2 was written 10 days ago, v2 became
		// noncurrent when v3 was written 3 days ago, v3 is current. As for
		// `Expiration`, the days are counted from the midnight that follows the
		// day a version became noncurrent, so v1 has been noncurrent for 9 days
		// and v2 for 2 days.
		let obj = object(vec![
			version(1, 30, 10),
			version(2, 10, 10),
			version(3, 3, 10),
		]);

		// After 10 days, nothing has been noncurrent for long enough
		assert!(expire(&obj, &after(10), &LifecycleFilter::default()).is_empty());

		// After 9 days, only v1 has
		assert_eq!(
			expire(&obj, &after(9), &LifecycleFilter::default()),
			vec![uuid(1)]
		);

		// After 2 days, both noncurrent versions have
		assert_eq!(
			expire(&obj, &after(2), &LifecycleFilter::default()),
			vec![uuid(1), uuid(2)]
		);
	}

	#[test]
	fn the_most_recent_noncurrent_versions_can_be_kept() {
		let obj = object(vec![
			version(1, 40, 10),
			version(2, 30, 10),
			version(3, 20, 10),
			version(4, 10, 10),
		]);
		let nv_exp = NoncurrentVersionExpiration {
			noncurrent_days: 1,
			newer_noncurrent_versions: Some(2),
		};
		// v1, v2 and v3 are noncurrent; the two most recent of them are kept
		assert_eq!(
			expire(&obj, &nv_exp, &LifecycleFilter::default()),
			vec![uuid(1)]
		);
	}

	#[test]
	fn locked_versions_are_never_expired() {
		let mut locked = version(1, 30, 10);
		locked.retention.update(Some(Retention {
			mode: ObjectLockMode::Compliance,
			retain_until: now_ms() + DAY_MS,
		}));
		let mut held = version(2, 30, 10);
		held.legal_hold.update(true);

		let obj = object(vec![locked, held, version(3, 20, 10), version(4, 5, 10)]);
		assert_eq!(
			expire(&obj, &after(1), &LifecycleFilter::default()),
			vec![uuid(3)]
		);
	}

	#[test]
	fn noncurrent_delete_markers_expire_too() {
		let obj = object(vec![delete_marker(1, 30), version(2, 20, 10)]);
		assert_eq!(
			expire(&obj, &after(1), &LifecycleFilter::default()),
			vec![uuid(1)]
		);
	}

	#[test]
	fn a_size_filter_leaves_delete_markers_alone() {
		let filter = LifecycleFilter {
			size_gt: Some(5),
			..Default::default()
		};
		// The delete marker has no size, so a rule with a size condition does
		// not apply to it; the small version does not match it either.
		let obj = object(vec![
			delete_marker(1, 30),
			version(2, 25, 1),
			version(3, 20, 100),
			version(4, 5, 10),
		]);
		assert_eq!(expire(&obj, &after(1), &filter), vec![uuid(3)]);
	}

	#[test]
	fn uploads_in_progress_are_not_noncurrent_versions() {
		let uploading = ObjectVersion::new(
			uuid(2),
			now_ms() - 30 * DAY_MS,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Uploading {
				multipart: true,
				checksum_algorithm: None,
				encryption: ObjectVersionEncryption::Plaintext {
					inner: ObjectVersionMetaInner {
						headers: vec![],
						checksum: None,
						checksum_type: None,
					},
				},
			},
		);
		let obj = object(vec![version(1, 40, 10), uploading, version(3, 20, 10)]);
		// Only v1 is a noncurrent version: the upload is not a version yet
		assert_eq!(
			expire(&obj, &after(1), &LifecycleFilter::default()),
			vec![uuid(1)]
		);
	}
}
