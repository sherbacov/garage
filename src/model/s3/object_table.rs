use serde::{Deserialize, Serialize};
use std::sync::Arc;

use garage_db as db;

use garage_util::data::*;
use garage_util::time::*;

use garage_table::crdt::*;
use garage_table::replication::TableShardedReplication;
use garage_table::*;

use crate::bucket_table::VersioningState;
use crate::index_counter::*;
use crate::s3::mpu_table::*;
use crate::s3::version_table::*;

/// The S3 version id of object versions that were created while bucket
/// versioning was disabled or suspended
pub const NULL_VERSION_ID: &str = "null";

pub const OBJECTS: &str = "objects";
pub const UNFINISHED_UPLOADS: &str = "unfinished_uploads";
pub const BYTES: &str = "bytes";

mod v08 {
	use garage_util::data::{Hash, Uuid};
	use serde::{Deserialize, Serialize};
	use std::collections::BTreeMap;

	/// An object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Object {
		/// The bucket in which the object is stored, used as partition key
		pub bucket_id: Uuid,

		/// The key at which the object is stored in its bucket, used as sorting key
		pub key: String,

		/// The list of currently stored versions of the object
		pub(super) versions: Vec<ObjectVersion>,
	}

	/// Information about a version of an object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersion {
		/// Id of the version
		pub uuid: Uuid,
		/// Timestamp of when the object was created
		pub timestamp: u64,
		/// State of the version
		pub state: ObjectVersionState,
	}

	/// State of an object version
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionState {
		/// The version is being received
		Uploading(ObjectVersionHeaders),
		/// The version is fully received
		Complete(ObjectVersionData),
		/// The version uploaded containded errors or the upload was explicitly aborted
		Aborted,
	}

	/// Data stored in object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionData {
		/// The object was deleted, this Version is a tombstone to mark it as such
		DeleteMarker,
		/// The object is short, it's stored inlined
		Inline(ObjectVersionMeta, #[serde(with = "serde_bytes")] Vec<u8>),
		/// The object is not short, Hash of first block is stored here, next segments hashes are
		/// stored in the version table
		FirstBlock(ObjectVersionMeta, Hash),
	}

	/// Metadata about the object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionMeta {
		/// Headers to send to the client
		pub headers: ObjectVersionHeaders,
		/// Size of the object
		pub size: u64,
		/// etag of the object
		pub etag: String,
	}

	/// Additional headers for an object
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionHeaders {
		/// Content type of the object
		pub content_type: String,
		/// Any other http headers to send
		pub other: BTreeMap<String, String>,
	}

	impl garage_util::migrate::InitialFormat for Object {}
}

mod v09 {
	use garage_util::data::Uuid;
	use serde::{Deserialize, Serialize};

	use super::v08;

	pub use v08::{ObjectVersionData, ObjectVersionHeaders, ObjectVersionMeta};

	/// An object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Object {
		/// The bucket in which the object is stored, used as partition key
		pub bucket_id: Uuid,

		/// The key at which the object is stored in its bucket, used as sorting key
		pub key: String,

		/// The list of currently stored versions of the object
		pub(super) versions: Vec<ObjectVersion>,
	}

	/// Information about a version of an object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersion {
		/// Id of the version
		pub uuid: Uuid,
		/// Timestamp of when the object was created
		pub timestamp: u64,
		/// State of the version
		pub state: ObjectVersionState,
	}

	/// State of an object version
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionState {
		/// The version is being received
		Uploading {
			/// Indicates whether this is a multipart upload
			multipart: bool,
			/// Headers to be included in the final object
			headers: ObjectVersionHeaders,
		},
		/// The version is fully received
		Complete(ObjectVersionData),
		/// The version uploaded containded errors or the upload was explicitly aborted
		Aborted,
	}

	impl garage_util::migrate::Migrate for Object {
		const VERSION_MARKER: &'static [u8] = b"G09s3o";

		type Previous = v08::Object;

		fn migrate(old: v08::Object) -> Object {
			let versions = old
				.versions
				.into_iter()
				.map(|x| ObjectVersion {
					uuid: x.uuid,
					timestamp: x.timestamp,
					state: match x.state {
						v08::ObjectVersionState::Uploading(h) => ObjectVersionState::Uploading {
							multipart: false,
							headers: h,
						},
						v08::ObjectVersionState::Complete(d) => ObjectVersionState::Complete(d),
						v08::ObjectVersionState::Aborted => ObjectVersionState::Aborted,
					},
				})
				.collect();
			Object {
				bucket_id: old.bucket_id,
				key: old.key,
				versions,
			}
		}
	}
}

mod v010 {
	use garage_util::data::{Hash, Uuid};
	use serde::{Deserialize, Serialize};

	use super::v09;

	/// An object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Object {
		/// The bucket in which the object is stored, used as partition key
		pub bucket_id: Uuid,

		/// The key at which the object is stored in its bucket, used as sorting key
		pub key: String,

		/// The list of currently stored versions of the object
		pub(super) versions: Vec<ObjectVersion>,
	}

	/// Information about a version of an object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersion {
		/// Id of the version
		pub uuid: Uuid,
		/// Timestamp of when the object was created
		pub timestamp: u64,
		/// State of the version
		pub state: ObjectVersionState,
	}

	/// State of an object version
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionState {
		/// The version is being received
		Uploading {
			/// Indicates whether this is a multipart upload
			multipart: bool,
			/// Checksum algorithm to use
			checksum_algorithm: Option<ChecksumAlgorithm>,
			/// Encryption params + headers to be included in the final object
			encryption: ObjectVersionEncryption,
		},
		/// The version is fully received
		Complete(ObjectVersionData),
		/// The version uploaded containded errors or the upload was explicitly aborted
		Aborted,
	}

	/// Data stored in object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionData {
		/// The object was deleted, this Version is a tombstone to mark it as such
		DeleteMarker,
		/// The object is short, it's stored inlined.
		/// It is never compressed. For encrypted objects, it is encrypted using
		/// AES256-GCM, like the encrypted headers.
		Inline(ObjectVersionMeta, #[serde(with = "serde_bytes")] Vec<u8>),
		/// The object is not short, Hash of first block is stored here, next segments hashes are
		/// stored in the version table
		FirstBlock(ObjectVersionMeta, Hash),
	}

	/// Metadata about the object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionMeta {
		/// Size of the object. If object is encrypted/compressed,
		/// this is always the size of the unencrypted/uncompressed data
		pub size: u64,
		/// etag of the object
		pub etag: String,
		/// Encryption params + headers (encrypted or plaintext)
		pub encryption: ObjectVersionEncryption,
	}

	/// Encryption information + metadata
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionEncryption {
		SseC {
			/// Encrypted serialized `ObjectVersionInner` struct.
			/// This is never compressed, just encrypted using AES256-GCM.
			#[serde(with = "serde_bytes")]
			inner: Vec<u8>,
			/// Whether data blocks are compressed in addition to being encrypted
			/// (compression happens before encryption, whereas for non-encrypted
			/// objects, compression is handled at the level of the block manager)
			compressed: bool,
			/// Whether the encryption uses an Object Encryption Key derived
			/// from the master SSE-C key, instead of the master SSE-C key itself.
			/// This is the case of objects created in Garage v2+.
			/// This field is kept for compatibility with Garage v2.0.0-beta1,
			/// which did not yet implement the v2 module below.
			#[serde(default)]
			use_oek: bool,
		},
		Plaintext {
			/// Plain-text headers
			inner: ObjectVersionMetaInner,
		},
	}

	/// Vector of headers, as tuples of the format (header name, header value)
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionMetaInner {
		pub headers: HeaderList,
		pub checksum: Option<ChecksumValue>,
	}

	pub type HeaderList = Vec<(String, String)>;

	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	pub enum ChecksumAlgorithm {
		Crc32,
		Crc32c,
		Crc64Nvme,
		Sha1,
		Sha256,
	}

	/// Checksum value for x-amz-checksum-algorithm
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub enum ChecksumValue {
		Crc32(#[serde(with = "serde_bytes")] [u8; 4]),
		Crc32c(#[serde(with = "serde_bytes")] [u8; 4]),
		Crc64Nvme(#[serde(with = "serde_bytes")] [u8; 8]),
		Sha1(#[serde(with = "serde_bytes")] [u8; 20]),
		Sha256(#[serde(with = "serde_bytes")] [u8; 32]),
	}

	impl garage_util::migrate::Migrate for Object {
		const VERSION_MARKER: &'static [u8] = b"G010s3ob";

		type Previous = v09::Object;

		fn migrate(old: v09::Object) -> Object {
			Object {
				bucket_id: old.bucket_id,
				key: old.key,
				versions: old.versions.into_iter().map(migrate_version).collect(),
			}
		}
	}

	fn migrate_version(old: v09::ObjectVersion) -> ObjectVersion {
		ObjectVersion {
			uuid: old.uuid,
			timestamp: old.timestamp,
			state: match old.state {
				v09::ObjectVersionState::Uploading { multipart, headers } => {
					ObjectVersionState::Uploading {
						multipart,
						checksum_algorithm: None,
						encryption: migrate_headers(headers),
					}
				}
				v09::ObjectVersionState::Complete(d) => {
					ObjectVersionState::Complete(migrate_data(d))
				}
				v09::ObjectVersionState::Aborted => ObjectVersionState::Aborted,
			},
		}
	}

	fn migrate_data(old: v09::ObjectVersionData) -> ObjectVersionData {
		match old {
			v09::ObjectVersionData::DeleteMarker => ObjectVersionData::DeleteMarker,
			v09::ObjectVersionData::Inline(meta, data) => {
				ObjectVersionData::Inline(migrate_meta(meta), data)
			}
			v09::ObjectVersionData::FirstBlock(meta, fb) => {
				ObjectVersionData::FirstBlock(migrate_meta(meta), fb)
			}
		}
	}

	fn migrate_meta(old: v09::ObjectVersionMeta) -> ObjectVersionMeta {
		ObjectVersionMeta {
			size: old.size,
			etag: old.etag,
			encryption: migrate_headers(old.headers),
		}
	}

	fn migrate_headers(old: v09::ObjectVersionHeaders) -> ObjectVersionEncryption {
		use http::header::CONTENT_TYPE;

		let mut new_headers = Vec::with_capacity(old.other.len() + 1);
		if old.content_type != "blob" {
			new_headers.push((CONTENT_TYPE.as_str().to_string(), old.content_type));
		}
		for (name, value) in old.other.into_iter() {
			new_headers.push((name, value));
		}

		ObjectVersionEncryption::Plaintext {
			inner: ObjectVersionMetaInner {
				headers: new_headers,
				checksum: None,
			},
		}
	}

	// Since ObjectVersionMetaInner can now be serialized independently, for the
	// purpose of being encrypted, we need it to support migrations on its own
	// as well.
	impl garage_util::migrate::InitialFormat for ObjectVersionMetaInner {
		const VERSION_MARKER: &'static [u8] = b"G010s3om";
	}
}

mod v2 {
	use garage_util::data::{Hash, Uuid};
	use garage_util::migrate::Migrate;
	use serde::{Deserialize, Serialize};

	use super::v010;
	pub use v010::{ChecksumAlgorithm, ChecksumValue};

	/// An object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Object {
		/// The bucket in which the object is stored, used as partition key
		pub bucket_id: Uuid,

		/// The key at which the object is stored in its bucket, used as sorting key
		pub key: String,

		/// The list of currently stored versions of the object
		pub(super) versions: Vec<ObjectVersion>,
	}

	/// Information about a version of an object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersion {
		/// Id of the version
		pub uuid: Uuid,
		/// Timestamp of when the object was created
		pub timestamp: u64,
		/// State of the version
		pub state: ObjectVersionState,
	}

	/// State of an object version
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionState {
		/// The version is being received
		Uploading {
			/// Indicates whether this is a multipart upload
			multipart: bool,
			/// Checksum algorithm and algorithm type to use
			checksum_algorithm: Option<(ChecksumAlgorithm, ChecksumType)>,
			/// Encryption params + headers to be included in the final object
			encryption: ObjectVersionEncryption,
		},
		/// The version is fully received
		Complete(ObjectVersionData),
		/// The version uploaded containded errors or the upload was explicitly aborted
		Aborted,
	}

	/// Data stored in object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionData {
		/// The object was deleted, this Version is a tombstone to mark it as such
		DeleteMarker,
		/// The object is short, it's stored inlined.
		/// It is never compressed. For encrypted objects, it is encrypted using
		/// AES256-GCM, like the encrypted headers.
		Inline(ObjectVersionMeta, #[serde(with = "serde_bytes")] Vec<u8>),
		/// The object is not short, Hash of first block is stored here, next segments hashes are
		/// stored in the version table
		FirstBlock(ObjectVersionMeta, Hash),
	}

	/// Metadata about the object version
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionMeta {
		/// Size of the object. If object is encrypted/compressed,
		/// this is always the size of the unencrypted/uncompressed data
		pub size: u64,
		/// etag of the object
		pub etag: String,
		/// Encryption params + headers (encrypted or plaintext)
		pub encryption: ObjectVersionEncryption,
	}

	/// Encryption information + metadata
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionEncryption {
		SseC {
			/// Encrypted serialized `ObjectVersionInner` struct.
			/// This is never compressed, just encrypted using AES256-GCM.
			#[serde(with = "serde_bytes")]
			inner: Vec<u8>,
			/// Whether data blocks are compressed in addition to being encrypted
			/// (compression happens before encryption, whereas for non-encrypted
			/// objects, compression is handled at the level of the block manager)
			compressed: bool,
			/// Whether the encryption uses an Object Encryption Key derived
			/// from the master SSE-C key, instead of the master SSE-C key itself.
			/// This is the case of objects created in Garage v2+
			use_oek: bool,
		},
		Plaintext {
			/// Plain-text headers
			inner: ObjectVersionMetaInner,
		},
	}

	/// Vector of headers, as tuples of the format (header name, header value)
	/// Note: checksum can be Some(_) with `checksum_type` = None for objects that
	/// have been migrated from Garage version before v2.0, as the distinction between
	/// full-object and composite checksums was not implemented yet.
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersionMetaInner {
		pub headers: HeaderList,
		pub checksum: Option<ChecksumValue>,
		// checksum_type has to be stored separately, because when migrating
		// from older versions of Garage, we can't know the correct value in
		// ObjectVersionMetaInner::migrate (because it cannot take an argument
		// that says whether the object was multipart or not)
		pub checksum_type: Option<ChecksumType>,
	}

	pub type HeaderList = Vec<(String, String)>;

	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	pub enum ChecksumType {
		FullObject,
		Composite,
	}

	impl garage_util::migrate::Migrate for Object {
		const VERSION_MARKER: &'static [u8] = b"G2s3ob";

		type Previous = v010::Object;

		fn migrate(old: v010::Object) -> Object {
			Object {
				bucket_id: old.bucket_id,
				key: old.key,
				versions: old.versions.into_iter().map(migrate_version).collect(),
			}
		}
	}

	fn migrate_version(old: v010::ObjectVersion) -> ObjectVersion {
		ObjectVersion {
			uuid: old.uuid,
			timestamp: old.timestamp,
			state: match old.state {
				v010::ObjectVersionState::Uploading {
					multipart,
					checksum_algorithm,
					encryption,
				} => ObjectVersionState::Uploading {
					multipart,
					checksum_algorithm: checksum_algorithm.map(|algo| match multipart {
						false => (algo, ChecksumType::FullObject),
						true => (algo, ChecksumType::Composite),
					}),
					encryption: migrate_encryption(encryption),
				},
				v010::ObjectVersionState::Complete(d) => {
					ObjectVersionState::Complete(migrate_data(d))
				}
				v010::ObjectVersionState::Aborted => ObjectVersionState::Aborted,
			},
		}
	}

	fn migrate_data(old: v010::ObjectVersionData) -> ObjectVersionData {
		match old {
			v010::ObjectVersionData::DeleteMarker => ObjectVersionData::DeleteMarker,
			v010::ObjectVersionData::Inline(meta, data) => {
				ObjectVersionData::Inline(migrate_meta(meta), data)
			}
			v010::ObjectVersionData::FirstBlock(meta, fb) => {
				ObjectVersionData::FirstBlock(migrate_meta(meta), fb)
			}
		}
	}

	fn migrate_meta(old: v010::ObjectVersionMeta) -> ObjectVersionMeta {
		ObjectVersionMeta {
			size: old.size,
			etag: old.etag,
			encryption: migrate_encryption(old.encryption),
		}
	}

	fn migrate_encryption(old: v010::ObjectVersionEncryption) -> ObjectVersionEncryption {
		match old {
			v010::ObjectVersionEncryption::SseC {
				inner,
				compressed,
				use_oek,
			} => ObjectVersionEncryption::SseC {
				inner,
				compressed,
				use_oek,
			},
			v010::ObjectVersionEncryption::Plaintext { inner } => {
				ObjectVersionEncryption::Plaintext {
					inner: ObjectVersionMetaInner::migrate(inner),
				}
			}
		}
	}

	impl Migrate for ObjectVersionMetaInner {
		const VERSION_MARKER: &'static [u8] = b"G2s3om";

		type Previous = v010::ObjectVersionMetaInner;

		fn migrate(old: v010::ObjectVersionMetaInner) -> ObjectVersionMetaInner {
			ObjectVersionMetaInner {
				headers: old.headers,
				checksum: old.checksum,
				checksum_type: None,
			}
		}
	}
}

pub(crate) mod v3 {
	use garage_util::crdt;
	use garage_util::data::Uuid;
	use serde::{Deserialize, Serialize};

	use super::v2;

	pub use v2::{
		ChecksumAlgorithm, ChecksumType, ChecksumValue, HeaderList, ObjectVersionData,
		ObjectVersionEncryption, ObjectVersionMeta, ObjectVersionMetaInner, ObjectVersionState,
	};

	/// An object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Object {
		/// The bucket in which the object is stored, used as partition key
		pub bucket_id: Uuid,

		/// The key at which the object is stored in its bucket, used as sorting key
		pub key: String,

		/// The list of currently stored versions of the object
		pub(super) versions: Vec<ObjectVersion>,
	}

	/// Information about a version of an object
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct ObjectVersion {
		/// Id of the version
		pub uuid: Uuid,
		/// Timestamp of when the object was created
		pub timestamp: u64,
		/// State of the version
		pub state: ObjectVersionState,
		/// Whether this version has its own S3 version id, or is the object's
		/// replaceable `null` version
		pub kind: ObjectVersionKind,
		/// Object Lock retention setting of this version
		pub retention: ObjectLockRetention,
		/// Object Lock legal hold status of this version
		pub legal_hold: crdt::Lww<bool>,
	}

	/// Whether an object version has its own S3 version id
	///
	/// The ordering of the variants is meaningful: if two nodes ever disagree,
	/// the version is kept rather than dropped.
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	pub enum ObjectVersionKind {
		/// The version was created while bucket versioning was disabled or
		/// suspended: its S3 version id is the special value `null`, and it is
		/// replaced by the next write to the same key
		Null,
		/// The version was created while bucket versioning was enabled: it has
		/// its own S3 version id and is kept until it is explicitly deleted
		Versioned,
	}

	/// The Object Lock retention setting of an object version
	///
	/// This is a last-write-wins register with an additional rule: a retention
	/// in compliance mode can never be lost, downgraded to governance mode, nor
	/// shortened, even by a concurrent write on another node.
	#[derive(PartialEq, Eq, Clone, Copy, Debug, Default, Serialize, Deserialize)]
	pub struct ObjectLockRetention {
		/// Timestamp of the last update, used for LWW reconciliation
		pub(super) ts: u64,
		/// The retention setting, if any
		pub(super) retention: Option<Retention>,
	}

	/// An Object Lock retention setting
	///
	/// The ordering of this type is meaningful: when two nodes disagree, the
	/// strictest setting wins.
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	pub struct Retention {
		pub mode: ObjectLockMode,
		/// Date until which the version is retained, in milliseconds since the
		/// UNIX epoch
		pub retain_until: u64,
	}

	/// The mode of an Object Lock retention setting
	///
	/// The ordering of the variants is meaningful: the strictest mode is the
	/// greatest one.
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub enum ObjectLockMode {
		/// The version can be deleted before `retain_until` by a caller that
		/// has the permission to bypass governance retention
		Governance,
		/// The version cannot be deleted by anyone before `retain_until`
		Compliance,
	}

	impl garage_util::migrate::Migrate for Object {
		const VERSION_MARKER: &'static [u8] = b"G3s3ob";

		type Previous = v2::Object;

		fn migrate(old: v2::Object) -> Object {
			Object {
				bucket_id: old.bucket_id,
				key: old.key,
				versions: old.versions.into_iter().map(migrate_version).collect(),
			}
		}
	}

	fn migrate_version(old: v2::ObjectVersion) -> ObjectVersion {
		ObjectVersion {
			uuid: old.uuid,
			timestamp: old.timestamp,
			state: old.state,
			// Objects that predate versioning support are all `null` versions,
			// and cannot be under Object Lock
			kind: ObjectVersionKind::Null,
			retention: ObjectLockRetention::default(),
			legal_hold: crdt::Lww::raw(0, false),
		}
	}
}

pub use v3::*;

impl Object {
	/// Initialize an Object struct from parts
	pub fn new(bucket_id: Uuid, key: String, versions: Vec<ObjectVersion>) -> Self {
		let mut ret = Self {
			bucket_id,
			key,
			versions: vec![],
		};
		for v in versions {
			ret.add_version(v)
				.expect("Twice the same ObjectVersion in Object constructor");
		}
		ret
	}

	/// Adds a version if it wasn't already present
	#[allow(clippy::result_unit_err)]
	pub fn add_version(&mut self, new: ObjectVersion) -> Result<(), ()> {
		match self
			.versions
			.binary_search_by(|v| v.cmp_key().cmp(&new.cmp_key()))
		{
			Err(i) => {
				self.versions.insert(i, new);
				Ok(())
			}
			Ok(_) => Err(()),
		}
	}

	/// Get a list of currently stored versions of `Object`
	pub fn versions(&self) -> &[ObjectVersion] {
		&self.versions[..]
	}

	/// The current version of the object: the most recent version that has been
	/// completely uploaded, if any. Note that this may be a delete marker.
	pub fn current_version(&self) -> Option<&ObjectVersion> {
		self.versions.iter().rev().find(|v| v.is_complete())
	}

	/// Get a completely uploaded version of this object by its S3 version id
	pub fn version_by_id(&self, version_id: &str) -> Option<&ObjectVersion> {
		self.versions
			.iter()
			.rev()
			.find(|v| v.is_complete() && v.version_id() == version_id)
	}

	/// Does the object currently exist, i.e. is its current version data
	/// rather than a delete marker
	pub fn is_data(&self) -> bool {
		self.current_version().map(|v| v.is_data()).unwrap_or(false)
	}
}

impl Crdt for ObjectVersionState {
	fn merge(&mut self, other: &Self) {
		use ObjectVersionState::*;
		match other {
			Aborted => {
				*self = Aborted;
			}
			Complete(b) => match self {
				Aborted => {}
				Complete(a) => {
					a.merge(b);
				}
				Uploading { .. } => {
					*self = Complete(b.clone());
				}
			},
			Uploading { .. } => {}
		}
	}
}

impl AutoCrdt for ObjectVersionData {
	const WARN_IF_DIFFERENT: bool = true;
}

impl ObjectVersionKind {
	/// The kind of the versions that are created by writes to a bucket that is
	/// in the given versioning state
	pub fn for_versioning_state(state: VersioningState) -> Self {
		match state {
			VersioningState::Enabled => Self::Versioned,
			VersioningState::Disabled | VersioningState::Suspended => Self::Null,
		}
	}
}

impl ObjectVersion {
	/// Create a new object version, with no Object Lock settings
	pub fn new(
		uuid: Uuid,
		timestamp: u64,
		kind: ObjectVersionKind,
		state: ObjectVersionState,
	) -> Self {
		Self {
			uuid,
			timestamp,
			state,
			kind,
			retention: ObjectLockRetention::default(),
			legal_hold: crdt::Lww::raw(0, false),
		}
	}

	fn cmp_key(&self) -> (u64, Uuid) {
		(self.timestamp, self.uuid)
	}

	/// Is the object version currently being uploaded
	///
	/// matches only multipart uploads if `check_multipart` is Some(true)
	/// matches only non-multipart uploads if `check_multipart` is Some(false)
	/// matches both if `check_multipart` is None
	pub fn is_uploading(&self, check_multipart: Option<bool>) -> bool {
		match &self.state {
			ObjectVersionState::Uploading { multipart, .. } => {
				check_multipart.map(|x| x == *multipart).unwrap_or(true)
			}
			_ => false,
		}
	}

	/// Is the object version completely received
	pub fn is_complete(&self) -> bool {
		matches!(self.state, ObjectVersionState::Complete(_))
	}

	/// Is the object version available (received and not a tombstone)
	pub fn is_data(&self) -> bool {
		match self.state {
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker) => false,
			ObjectVersionState::Complete(_) => true,
			_ => false,
		}
	}

	/// Is the object version a delete marker
	pub fn is_delete_marker(&self) -> bool {
		matches!(
			self.state,
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker)
		)
	}

	/// Is this version kept even when a more recent version of the object exists
	///
	/// Versions created while bucket versioning was enabled are kept until they
	/// are explicitly deleted, at which point they are put in the `Aborted`
	/// state so that the data they reference is garbage-collected.
	pub fn is_retained(&self) -> bool {
		self.kind == ObjectVersionKind::Versioned
			&& !matches!(self.state, ObjectVersionState::Aborted)
	}

	/// What prevents this version from being permanently deleted at the given
	/// date, if anything
	pub fn delete_protection(&self, timestamp: u64) -> DeleteProtection {
		// A legal hold protects a version independently of its retention
		// setting, and can never be bypassed.
		if *self.legal_hold.get() {
			return DeleteProtection::LegalHold;
		}
		match self.retention.get() {
			Some(r) if r.is_active_at(timestamp) => match r.mode {
				ObjectLockMode::Governance => DeleteProtection::Governance {
					retain_until: r.retain_until,
				},
				ObjectLockMode::Compliance => DeleteProtection::Compliance {
					retain_until: r.retain_until,
				},
			},
			_ => DeleteProtection::None,
		}
	}

	/// The S3 version id of this version
	pub fn version_id(&self) -> String {
		match self.kind {
			ObjectVersionKind::Null => NULL_VERSION_ID.to_string(),
			ObjectVersionKind::Versioned => hex::encode(self.uuid),
		}
	}
}

/// What prevents an object version from being permanently deleted
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum DeleteProtection {
	/// Nothing: the version can be deleted
	None,
	/// The version is under a legal hold, and cannot be deleted by anyone
	/// until the hold is lifted
	LegalHold,
	/// The version is retained in governance mode, and can only be deleted by
	/// a caller that is allowed to bypass governance retention
	Governance { retain_until: u64 },
	/// The version is retained in compliance mode, and cannot be deleted by
	/// anyone before `retain_until`
	Compliance { retain_until: u64 },
}

impl DeleteProtection {
	/// Is the version protected against a caller that does not bypass
	/// governance retention
	pub fn is_protected(&self) -> bool {
		*self != DeleteProtection::None
	}

	/// Is the version protected even against a caller that bypasses governance
	/// retention
	pub fn is_protected_from_bypass(&self) -> bool {
		match self {
			DeleteProtection::None | DeleteProtection::Governance { .. } => false,
			DeleteProtection::LegalHold | DeleteProtection::Compliance { .. } => true,
		}
	}
}

impl Crdt for ObjectVersion {
	fn merge(&mut self, other: &Self) {
		self.state.merge(&other.state);
		self.kind.merge(&other.kind);
		self.retention.merge(&other.retention);
		self.legal_hold.merge(&other.legal_hold);
	}
}

impl AutoCrdt for ObjectVersionKind {
	const WARN_IF_DIFFERENT: bool = true;
}

impl ObjectLockRetention {
	/// A retention setting that is updated now
	pub fn new(retention: Option<Retention>) -> Self {
		Self {
			ts: now_msec(),
			retention,
		}
	}

	/// The retention setting, if any
	pub fn get(&self) -> Option<&Retention> {
		self.retention.as_ref()
	}

	/// Update the retention setting, keeping causal ordering (see `crdt::Lww`)
	pub fn update(&mut self, retention: Option<Retention>) {
		self.ts = std::cmp::max(self.ts + 1, now_msec());
		self.retention = retention;
	}

	/// The date until which this version is retained in compliance mode, if it is
	fn compliance_retain_until(&self) -> Option<u64> {
		match self.retention {
			Some(Retention {
				mode: ObjectLockMode::Compliance,
				retain_until,
			}) => Some(retain_until),
			_ => None,
		}
	}
}

impl Crdt for ObjectLockRetention {
	fn merge(&mut self, other: &Self) {
		// A retention in compliance mode can never be lost, downgraded to
		// governance mode, nor shortened: whenever either side is in compliance
		// mode, keep the strictest of the two, whatever the timestamps say.
		let compliance = std::cmp::max(
			self.compliance_retain_until(),
			other.compliance_retain_until(),
		);
		if let Some(retain_until) = compliance {
			*self = ObjectLockRetention {
				ts: std::cmp::max(self.ts, other.ts),
				retention: Some(Retention {
					mode: ObjectLockMode::Compliance,
					retain_until,
				}),
			};
			return;
		}

		// Otherwise, governance-mode retention behaves as a LWW register, with
		// the strictest setting as a deterministic tie-break.
		match self.ts.cmp(&other.ts) {
			std::cmp::Ordering::Less => *self = *other,
			std::cmp::Ordering::Greater => (),
			std::cmp::Ordering::Equal => {
				if other.retention > self.retention {
					self.retention = other.retention;
				}
			}
		}
	}
}

impl Retention {
	/// Is this retention setting still in effect at the given date
	pub fn is_active_at(&self, timestamp: u64) -> bool {
		self.retain_until > timestamp
	}
}

impl Entry<Uuid, String> for Object {
	fn partition_key(&self) -> &Uuid {
		&self.bucket_id
	}
	fn sort_key(&self) -> &String {
		&self.key
	}
	fn is_tombstone(&self) -> bool {
		// A delete marker that is a retained S3 version is not a tombstone: it
		// is listed by ListObjectVersions and can be deleted to restore the
		// previous version of the object.
		self.versions.len() == 1
			&& self.versions[0].kind == ObjectVersionKind::Null
			&& self.versions[0].state
				== ObjectVersionState::Complete(ObjectVersionData::DeleteMarker)
	}
}

impl ChecksumValue {
	pub fn algorithm(&self) -> ChecksumAlgorithm {
		match self {
			ChecksumValue::Crc32(_) => ChecksumAlgorithm::Crc32,
			ChecksumValue::Crc32c(_) => ChecksumAlgorithm::Crc32c,
			ChecksumValue::Crc64Nvme(_) => ChecksumAlgorithm::Crc64Nvme,
			ChecksumValue::Sha1(_) => ChecksumAlgorithm::Sha1,
			ChecksumValue::Sha256(_) => ChecksumAlgorithm::Sha256,
		}
	}
}

impl Crdt for Object {
	fn merge(&mut self, other: &Self) {
		// Merge versions from other into here
		for other_v in other.versions.iter() {
			match self
				.versions
				.binary_search_by(|v| v.cmp_key().cmp(&other_v.cmp_key()))
			{
				Ok(i) => {
					self.versions[i].merge(other_v);
				}
				Err(i) => {
					self.versions.insert(i, other_v.clone());
				}
			}
		}

		// Remove versions which are obsolete, i.e. those that come before the
		// last version which .is_complete(), with the exception of the versions
		// that are retained as distinct S3 versions: those were created while
		// bucket versioning was enabled, and are kept until they are explicitly
		// deleted.
		let last_complete = self
			.versions
			.iter()
			.enumerate()
			.rev()
			.find(|(_, v)| v.is_complete())
			.map(|(vi, _)| vi);

		if let Some(last_vi) = last_complete {
			let mut vi = 0;
			self.versions.retain(|v| {
				let keep = vi >= last_vi || v.is_retained();
				vi += 1;
				keep
			});
		}
	}
}

pub struct ObjectTable {
	pub version_table: Arc<Table<VersionTable, TableShardedReplication>>,
	pub mpu_table: Arc<Table<MultipartUploadTable, TableShardedReplication>>,
	pub object_counter_table: Arc<IndexCounter<Object>>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ObjectFilter {
	/// Is the object version available (received and not a tombstone)
	IsData,
	/// Is the object version currently being uploaded
	///
	/// matches only multipart uploads if `check_multipart` is Some(true)
	/// matches only non-multipart uploads if `check_multipart` is Some(false)
	/// matches both if `check_multipart` is None
	IsUploading { check_multipart: Option<bool> },
	/// Does the object have at least one version that has been completely
	/// uploaded, which includes delete markers
	///
	/// This is what `ListObjectVersions` lists, as opposed to `IsData` which
	/// only matches objects that currently exist.
	HasVersions,
	/// Does the object have at least one version that is retained as a
	/// distinct S3 version, which includes delete markers
	HasRetainedVersions,
}

impl TableSchema for ObjectTable {
	const TABLE_NAME: &'static str = "object";

	type P = Uuid;
	type S = String;
	type E = Object;
	type Filter = ObjectFilter;

	fn updated(
		&self,
		tx: &mut db::Transaction,
		old: Option<&Self::E>,
		new: Option<&Self::E>,
	) -> db::TxOpResult<()> {
		// 1. Count
		let counter_res = self.object_counter_table.count(tx, old, new);
		if let Err(e) = db::unabort(counter_res)? {
			error!(
				"Unable to update object counter: {}. Index values will be wrong!",
				e
			);
		}

		// 2. Enqueue propagation deletions to version table
		if let (Some(old_v), Some(new_v)) = (old, new) {
			for v in old_v.versions.iter() {
				let new_v_id = new_v
					.versions
					.binary_search_by(|nv| nv.cmp_key().cmp(&v.cmp_key()));

				// Propagate deletion of old versions to the Version table
				let delete_version = match new_v_id {
					Err(_) => true,
					Ok(i) => {
						new_v.versions[i].state == ObjectVersionState::Aborted
							&& v.state != ObjectVersionState::Aborted
					}
				};
				if delete_version {
					let deleted_version = Version::new(
						v.uuid,
						VersionBacklink::Object {
							bucket_id: old_v.bucket_id,
							key: old_v.key.clone(),
						},
						true,
					);
					let res = self.version_table.queue_insert(tx, &deleted_version);
					if let Err(e) = db::unabort(res)? {
						error!(
							"Unable to enqueue version deletion propagation: {}. A repair will be needed.",
							e
						);
					}
				}

				// After abortion or completion of multipart uploads, delete MPU table entry
				if matches!(
					v.state,
					ObjectVersionState::Uploading {
						multipart: true,
						..
					}
				) {
					let delete_mpu = match new_v_id {
						Err(_) => true,
						Ok(i) => !matches!(
							new_v.versions[i].state,
							ObjectVersionState::Uploading { .. }
						),
					};
					if delete_mpu {
						let deleted_mpu = MultipartUpload::new(
							v.uuid,
							v.timestamp,
							old_v.bucket_id,
							old_v.key.clone(),
							true,
						);
						let res = self.mpu_table.queue_insert(tx, &deleted_mpu);
						if let Err(e) = db::unabort(res)? {
							error!(
								"Unable to enqueue multipart upload deletion propagation: {}. A repair will be needed.",
								e
							);
						}
					}
				}
			}
		}

		Ok(())
	}

	fn matches_filter(entry: &Self::E, filter: &Self::Filter) -> bool {
		match filter {
			ObjectFilter::IsData => entry.is_data(),
			ObjectFilter::HasVersions => entry.versions.iter().any(|v| v.is_complete()),
			ObjectFilter::HasRetainedVersions => entry.versions.iter().any(|v| v.is_retained()),
			ObjectFilter::IsUploading { check_multipart } => entry
				.versions
				.iter()
				.any(|v| v.is_uploading(*check_multipart)),
		}
	}
}

impl CountedItem for Object {
	const COUNTER_TABLE_NAME: &'static str = "bucket_object_counter";

	// Partition key = bucket id
	type CP = Uuid;
	// Sort key = nothing
	type CS = EmptyKey;

	fn counter_partition_key(&self) -> &Uuid {
		&self.bucket_id
	}
	fn counter_sort_key(&self) -> &EmptyKey {
		&EmptyKey
	}

	fn counts(&self) -> Vec<(&'static str, i64)> {
		let versions = self.versions();
		let n_objects = if self.is_data() { 1 } else { 0 };
		let n_unfinished_uploads = versions.iter().filter(|v| v.is_uploading(None)).count();
		let n_bytes = versions
			.iter()
			.map(|v| match &v.state {
				ObjectVersionState::Complete(ObjectVersionData::Inline(meta, _))
				| ObjectVersionState::Complete(ObjectVersionData::FirstBlock(meta, _)) => meta.size,
				_ => 0,
			})
			.sum::<u64>();

		vec![
			(OBJECTS, n_objects),
			(UNFINISHED_UPLOADS, n_unfinished_uploads as i64),
			(BYTES, n_bytes as i64),
		]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const KEY: &str = "the/key";

	fn uuid(n: u8) -> Uuid {
		Uuid::from([n; 32])
	}

	fn bucket() -> Uuid {
		uuid(0x42)
	}

	fn data_state(size: u64) -> ObjectVersionState {
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
		))
	}

	fn uploading_state() -> ObjectVersionState {
		ObjectVersionState::Uploading {
			multipart: false,
			checksum_algorithm: None,
			encryption: ObjectVersionEncryption::Plaintext {
				inner: ObjectVersionMetaInner {
					headers: vec![],
					checksum: None,
					checksum_type: None,
				},
			},
		}
	}

	fn version(n: u8, kind: ObjectVersionKind, state: ObjectVersionState) -> ObjectVersion {
		ObjectVersion::new(uuid(n), 1000 + n as u64, kind, state)
	}

	fn object(versions: Vec<ObjectVersion>) -> Object {
		Object::new(bucket(), KEY.into(), versions)
	}

	/// Merge `b` into `a` and return the version ids that `a` is left with
	fn merged(a: Object, b: Object) -> Vec<Uuid> {
		let mut a = a;
		a.merge(&b);
		a.versions().iter().map(|v| v.uuid).collect()
	}

	#[test]
	fn null_versions_replace_each_other() {
		// This is how a bucket on which versioning was never enabled behaves:
		// writing an object drops the version that was there before.
		let old = object(vec![version(1, ObjectVersionKind::Null, data_state(10))]);
		let new = object(vec![version(2, ObjectVersionKind::Null, data_state(20))]);
		assert_eq!(merged(old, new), vec![uuid(2)]);
	}

	#[test]
	fn versioned_versions_are_all_kept() {
		let old = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			data_state(10),
		)]);
		let new = object(vec![version(
			2,
			ObjectVersionKind::Versioned,
			data_state(20),
		)]);
		assert_eq!(merged(old, new), vec![uuid(1), uuid(2)]);
	}

	#[test]
	fn merge_is_commutative() {
		let old = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			data_state(10),
		)]);
		let new = object(vec![version(
			2,
			ObjectVersionKind::Versioned,
			data_state(20),
		)]);
		assert_eq!(
			merged(old.clone(), new.clone()),
			merged(new.clone(), old.clone())
		);

		// ... and idempotent
		let mut once = old.clone();
		once.merge(&new);
		let mut twice = once.clone();
		twice.merge(&new);
		assert_eq!(once, twice);
	}

	#[test]
	fn suspended_versioning_only_replaces_the_null_version() {
		// A bucket whose versioning is suspended writes `null` versions, which
		// replace each other but leave the versions written while versioning
		// was enabled alone.
		let existing = object(vec![
			version(1, ObjectVersionKind::Versioned, data_state(10)),
			version(2, ObjectVersionKind::Null, data_state(20)),
		]);
		let new = object(vec![version(3, ObjectVersionKind::Null, data_state(30))]);
		assert_eq!(merged(existing, new), vec![uuid(1), uuid(3)]);
	}

	#[test]
	fn deleting_a_version_removes_it() {
		// Permanently deleting one version of an object puts it in the Aborted
		// state, and it is dropped as soon as a later complete version exists.
		let existing = object(vec![
			version(1, ObjectVersionKind::Versioned, data_state(10)),
			version(2, ObjectVersionKind::Versioned, data_state(20)),
		]);
		let deleted = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Aborted,
		)]);
		assert_eq!(merged(existing, deleted), vec![uuid(2)]);
	}

	#[test]
	fn a_delete_marker_hides_previous_versions_without_dropping_them() {
		let existing = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			data_state(10),
		)]);
		let marker = object(vec![version(
			2,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
		)]);

		let mut obj = existing;
		obj.merge(&marker);

		assert_eq!(
			obj.versions().iter().map(|v| v.uuid).collect::<Vec<_>>(),
			vec![uuid(1), uuid(2)]
		);
		assert_eq!(obj.current_version().map(|v| v.uuid), Some(uuid(2)));
		assert!(!obj.is_data());
		assert!(!obj.is_tombstone());
	}

	#[test]
	fn a_null_delete_marker_alone_is_a_tombstone() {
		// On a bucket that was never versioned, an object that has been deleted
		// can be garbage-collected; a delete marker that is a real S3 version
		// cannot, as it is listed and can be deleted to restore the object.
		let deleted = object(vec![version(
			1,
			ObjectVersionKind::Null,
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
		)]);
		assert!(deleted.is_tombstone());

		let deleted = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			ObjectVersionState::Complete(ObjectVersionData::DeleteMarker),
		)]);
		assert!(!deleted.is_tombstone());
	}

	#[test]
	fn an_ongoing_versioned_upload_survives_a_concurrent_write() {
		// On a versioned bucket, a multipart upload that was started before
		// another write completed must not be dropped, as completing it will
		// create a version of its own.
		let uploading = object(vec![version(
			1,
			ObjectVersionKind::Versioned,
			uploading_state(),
		)]);
		let written = object(vec![version(
			2,
			ObjectVersionKind::Versioned,
			data_state(20),
		)]);
		assert_eq!(merged(uploading, written), vec![uuid(1), uuid(2)]);

		// On a bucket that is not versioned, it is dropped, as it was before
		// Garage supported versioning.
		let uploading = object(vec![version(1, ObjectVersionKind::Null, uploading_state())]);
		let written = object(vec![version(2, ObjectVersionKind::Null, data_state(20))]);
		assert_eq!(merged(uploading, written), vec![uuid(2)]);
	}

	#[test]
	fn version_ids() {
		assert_eq!(
			version(1, ObjectVersionKind::Null, data_state(1)).version_id(),
			NULL_VERSION_ID
		);
		assert_eq!(
			version(1, ObjectVersionKind::Versioned, data_state(1)).version_id(),
			hex::encode(uuid(1))
		);
	}

	#[test]
	fn lookup_by_version_id() {
		let obj = object(vec![
			version(1, ObjectVersionKind::Versioned, data_state(10)),
			version(2, ObjectVersionKind::Null, data_state(20)),
		]);
		assert_eq!(
			obj.version_by_id(&hex::encode(uuid(1))).map(|v| v.uuid),
			Some(uuid(1))
		);
		assert_eq!(
			obj.version_by_id(NULL_VERSION_ID).map(|v| v.uuid),
			Some(uuid(2))
		);
		assert!(obj.version_by_id(&hex::encode(uuid(3))).is_none());
	}

	fn retention(mode: ObjectLockMode, retain_until: u64) -> ObjectLockRetention {
		ObjectLockRetention {
			ts: 100,
			retention: Some(Retention { mode, retain_until }),
		}
	}

	#[test]
	fn governance_retention_is_last_write_wins() {
		let mut a = retention(ObjectLockMode::Governance, 5000);
		let mut b = ObjectLockRetention {
			ts: 200,
			retention: None,
		};
		let (a0, b0) = (a, b);

		a.merge(&b0);
		b.merge(&a0);
		assert_eq!(a, b);
		// The more recent write wins, even though it lifts the retention
		assert_eq!(a.get(), None);
	}

	#[test]
	fn compliance_retention_can_never_be_lifted_or_shortened() {
		let compliance = retention(ObjectLockMode::Compliance, 5000);

		// A concurrent, more recent write that lifts the retention loses
		let mut merged = compliance;
		merged.merge(&ObjectLockRetention {
			ts: 999,
			retention: None,
		});
		assert_eq!(
			merged.get(),
			Some(&Retention {
				mode: ObjectLockMode::Compliance,
				retain_until: 5000
			})
		);

		// So does one that shortens it or downgrades it to governance mode
		let mut merged = compliance;
		merged.merge(&ObjectLockRetention {
			ts: 999,
			retention: Some(Retention {
				mode: ObjectLockMode::Governance,
				retain_until: 1,
			}),
		});
		assert_eq!(
			merged.get(),
			Some(&Retention {
				mode: ObjectLockMode::Compliance,
				retain_until: 5000
			})
		);

		// But it can be extended
		let mut merged = compliance;
		merged.merge(&retention(ObjectLockMode::Compliance, 9000));
		assert_eq!(merged.get().unwrap().retain_until, 9000);
	}

	#[test]
	fn delete_protection_of_a_version() {
		let now = 1_000_000;
		let mut v = version(1, ObjectVersionKind::Versioned, data_state(10));
		assert_eq!(v.delete_protection(now), DeleteProtection::None);

		// An expired retention does not protect the version any more
		v.retention.update(Some(Retention {
			mode: ObjectLockMode::Governance,
			retain_until: now - 1,
		}));
		assert_eq!(v.delete_protection(now), DeleteProtection::None);

		v.retention.update(Some(Retention {
			mode: ObjectLockMode::Governance,
			retain_until: now + 1,
		}));
		assert_eq!(
			v.delete_protection(now),
			DeleteProtection::Governance {
				retain_until: now + 1
			}
		);
		assert!(v.delete_protection(now).is_protected());
		assert!(!v.delete_protection(now).is_protected_from_bypass());

		v.retention.update(Some(Retention {
			mode: ObjectLockMode::Compliance,
			retain_until: now + 1,
		}));
		assert!(v.delete_protection(now).is_protected_from_bypass());

		// A legal hold protects the version whatever its retention says
		v.retention.update(None);
		v.legal_hold.update(true);
		assert_eq!(v.delete_protection(now), DeleteProtection::LegalHold);
		assert!(v.delete_protection(now).is_protected_from_bypass());

		v.legal_hold.update(false);
		assert_eq!(v.delete_protection(now), DeleteProtection::None);
	}

	#[test]
	fn locks_survive_a_merge_with_a_version_that_has_none() {
		// Writing an object version again (e.g. to mark an upload as complete)
		// must not clear the lock settings that were set on it in the meantime.
		let mut locked = version(1, ObjectVersionKind::Versioned, data_state(10));
		locked.retention.update(Some(Retention {
			mode: ObjectLockMode::Compliance,
			retain_until: 9_000,
		}));
		locked.legal_hold.update(true);

		let plain = version(1, ObjectVersionKind::Versioned, data_state(10));

		let mut obj = object(vec![locked]);
		obj.merge(&object(vec![plain]));

		let v = &obj.versions()[0];
		assert_eq!(v.retention.get().unwrap().retain_until, 9_000);
		assert!(*v.legal_hold.get());
	}

	#[test]
	fn compliance_retention_merge_is_commutative() {
		let a = retention(ObjectLockMode::Compliance, 5000);
		let b = ObjectLockRetention {
			ts: 999,
			retention: None,
		};

		let mut ab = a;
		ab.merge(&b);
		let mut ba = b;
		ba.merge(&a);
		assert_eq!(ab, ba);
	}
}
