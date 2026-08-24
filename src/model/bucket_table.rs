use garage_table::crdt::*;
use garage_table::*;
use garage_util::data::*;
use garage_util::time::*;

use crate::permission::BucketKeyPerm;

mod v08 {
	use crate::permission::BucketKeyPerm;
	use garage_util::crdt;
	use garage_util::data::Uuid;
	use serde::{Deserialize, Serialize};

	/// A bucket is a collection of objects
	///
	/// Its parameters are not directly accessible as:
	///  - It must be possible to merge parameters, hence the use of a LWW CRDT.
	///  - A bucket has 2 states, Present or Deleted and parameters make sense only if present.
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Bucket {
		/// ID of the bucket
		pub id: Uuid,
		/// State, and configuration if not deleted, of the bucket
		pub state: crdt::Deletable<BucketParams>,
	}

	/// Configuration for a bucket
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct BucketParams {
		/// Bucket's creation date
		pub creation_date: u64,
		/// Map of key with access to the bucket, and what kind of access they give
		pub authorized_keys: crdt::Map<String, BucketKeyPerm>,

		/// Map of aliases that are or have been given to this bucket
		/// in the global namespace
		/// (not authoritative: this is just used as an indication to
		/// map back to aliases when doing `ListBuckets`)
		pub aliases: crdt::LwwMap<String, bool>,
		/// Map of aliases that are or have been given to this bucket
		/// in namespaces local to keys
		/// key = (access key id, alias name)
		pub local_aliases: crdt::LwwMap<(String, String), bool>,

		/// Whether this bucket is allowed for website access
		/// (under all of its global alias names),
		/// and if so, the website configuration XML document
		pub website_config: crdt::Lww<crdt::CancelingOption<WebsiteConfig>>,
		/// CORS rules
		pub cors_config: crdt::Lww<crdt::CancelingOption<Vec<CorsRule>>>,
		/// Lifecycle configuration
		#[serde(default)]
		pub lifecycle_config: crdt::Lww<crdt::CancelingOption<Vec<LifecycleRule>>>,
		/// Bucket quotas
		#[serde(default)]
		pub quotas: crdt::Lww<BucketQuotas>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct WebsiteConfig {
		pub index_document: String,
		pub error_document: Option<String>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct CorsRule {
		pub id: Option<String>,
		pub max_age_seconds: Option<u64>,
		pub allow_origins: Vec<String>,
		pub allow_methods: Vec<String>,
		pub allow_headers: Vec<String>,
		pub expose_headers: Vec<String>,
	}

	/// Lifecycle configuration rule
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct LifecycleRule {
		/// The ID of the rule
		pub id: Option<String>,
		/// Whether the rule is active
		pub enabled: bool,
		/// The filter to check whether rule applies to a given object
		pub filter: LifecycleFilter,
		/// Number of days after which incomplete multipart uploads are aborted
		pub abort_incomplete_mpu_days: Option<usize>,
		/// Expiration policy for stored objects
		pub expiration: Option<LifecycleExpiration>,
	}

	/// A lifecycle filter is a set of conditions that must all be true.
	/// For each condition, if it is None, it is not verified (always true),
	/// and if it is Some(x), then it is verified for value x
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, Default)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct LifecycleFilter {
		/// If Some(x), object key has to start with prefix x
		pub prefix: Option<String>,
		/// If Some(x), object size has to be more than x
		pub size_gt: Option<u64>,
		/// If Some(x), object size has to be less than x
		pub size_lt: Option<u64>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub enum LifecycleExpiration {
		/// Objects expire x days after they were created
		AfterDays(usize),
		/// Objects expire at date x (must be in yyyy-mm-dd format)
		AtDate(String),
	}

	#[derive(Default, PartialEq, Eq, PartialOrd, Ord, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct BucketQuotas {
		/// Maximum size in bytes (bucket size = sum of sizes of objects in the bucket)
		pub max_size: Option<u64>,
		/// Maximum number of non-deleted objects in the bucket
		pub max_objects: Option<u64>,
	}

	impl garage_util::migrate::InitialFormat for Bucket {}
}

mod v2 {
	use crate::permission::BucketKeyPerm;
	use garage_util::crdt;
	use garage_util::data::Uuid;
	use serde::{Deserialize, Serialize};

	use super::v08;

	pub use v08::{BucketQuotas, CorsRule, LifecycleExpiration, LifecycleFilter, LifecycleRule};

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Bucket {
		/// ID of the bucket
		pub id: Uuid,
		/// State, and configuration if not deleted, of the bucket
		pub state: crdt::Deletable<BucketParams>,
	}

	/// Configuration for a bucket
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct BucketParams {
		/// Bucket's creation date
		pub creation_date: u64,
		/// Map of key with access to the bucket, and what kind of access they give
		pub authorized_keys: crdt::Map<String, BucketKeyPerm>,

		/// Map of aliases that are or have been given to this bucket
		/// in the global namespace
		/// (not authoritative: this is just used as an indication to
		/// map back to aliases when doing `ListBuckets`)
		pub aliases: crdt::LwwMap<String, bool>,
		/// Map of aliases that are or have been given to this bucket
		/// in namespaces local to keys
		/// key = (access key id, alias name)
		pub local_aliases: crdt::LwwMap<(String, String), bool>,

		/// Whether this bucket is allowed for website access
		/// (under all of its global alias names),
		/// and if so, the website configuration XML document
		pub website_config: crdt::Lww<crdt::CancelingOption<WebsiteConfig>>,
		/// CORS rules
		pub cors_config: crdt::Lww<crdt::CancelingOption<Vec<CorsRule>>>,
		/// Lifecycle configuration
		pub lifecycle_config: crdt::Lww<crdt::CancelingOption<Vec<LifecycleRule>>>,
		/// Bucket quotas
		pub quotas: crdt::Lww<BucketQuotas>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct WebsiteConfig {
		pub index_document: String,
		pub error_document: Option<String>,
		// this field is currently unused, but present so adding it in the future doesn't
		// need a new migration
		pub redirect_all: Option<RedirectAll>,
		pub routing_rules: Vec<RoutingRule>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct RedirectAll {
		pub hostname: String,
		pub protocol: String,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct RoutingRule {
		pub condition: Option<RedirectCondition>,
		pub redirect: Redirect,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct RedirectCondition {
		pub http_error_code: Option<u16>,
		pub prefix: Option<String>,
	}

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct Redirect {
		pub hostname: Option<String>,
		pub http_redirect_code: u16,
		pub protocol: Option<String>,
		pub replace_key_prefix: Option<String>,
		pub replace_key: Option<String>,
	}

	impl garage_util::migrate::Migrate for Bucket {
		const VERSION_MARKER: &'static [u8] = b"G2bkt";

		type Previous = v08::Bucket;

		fn migrate(old: v08::Bucket) -> Bucket {
			Bucket {
				id: old.id,
				state: old.state.map(|x| BucketParams {
					creation_date: x.creation_date,
					authorized_keys: x.authorized_keys,
					aliases: x.aliases,
					local_aliases: x.local_aliases,
					website_config: x.website_config.map(|wc_opt| {
						wc_opt.map(|wc| WebsiteConfig {
							index_document: wc.index_document,
							error_document: wc.error_document,
							redirect_all: None,
							routing_rules: vec![],
						})
					}),
					cors_config: x.cors_config,
					lifecycle_config: x.lifecycle_config,
					quotas: x.quotas,
				}),
			}
		}
	}
}

mod v3 {
	use crate::permission::BucketKeyPerm;
	use crate::s3::object_table::v3::ObjectLockMode;
	use garage_util::crdt;
	use garage_util::data::Uuid;
	use serde::{Deserialize, Serialize};

	use super::v2;

	pub use v2::{
		BucketQuotas, CorsRule, LifecycleExpiration, LifecycleFilter, Redirect, RedirectAll,
		RedirectCondition, RoutingRule, WebsiteConfig,
	};

	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	pub struct Bucket {
		/// ID of the bucket
		pub id: Uuid,
		/// State, and configuration if not deleted, of the bucket
		pub state: crdt::Deletable<BucketParams>,
	}

	/// Configuration for a bucket
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct BucketParams {
		/// Bucket's creation date
		pub creation_date: u64,
		/// Map of key with access to the bucket, and what kind of access they give
		pub authorized_keys: crdt::Map<String, BucketKeyPerm>,

		/// Map of aliases that are or have been given to this bucket
		/// in the global namespace
		/// (not authoritative: this is just used as an indication to
		/// map back to aliases when doing `ListBuckets`)
		pub aliases: crdt::LwwMap<String, bool>,
		/// Map of aliases that are or have been given to this bucket
		/// in namespaces local to keys
		/// key = (access key id, alias name)
		pub local_aliases: crdt::LwwMap<(String, String), bool>,

		/// Whether this bucket is allowed for website access
		/// (under all of its global alias names),
		/// and if so, the website configuration XML document
		pub website_config: crdt::Lww<crdt::CancelingOption<WebsiteConfig>>,
		/// CORS rules
		pub cors_config: crdt::Lww<crdt::CancelingOption<Vec<CorsRule>>>,
		/// Lifecycle configuration
		pub lifecycle_config: crdt::Lww<crdt::CancelingOption<Vec<LifecycleRule>>>,
		/// Bucket quotas
		pub quotas: crdt::Lww<BucketQuotas>,
		/// Whether object versioning is enabled on this bucket
		pub versioning: crdt::Lww<VersioningState>,
		/// Object Lock configuration, if Object Lock is enabled on this bucket
		pub object_lock: crdt::Lww<crdt::CancelingOption<ObjectLockConfig>>,
	}

	/// Lifecycle configuration rule
	#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct LifecycleRule {
		/// The ID of the rule
		pub id: Option<String>,
		/// Whether the rule is active
		pub enabled: bool,
		/// The filter to check whether rule applies to a given object
		pub filter: LifecycleFilter,
		/// Number of days after which incomplete multipart uploads are aborted
		pub abort_incomplete_mpu_days: Option<usize>,
		/// Expiration policy for the current version of stored objects
		pub expiration: Option<LifecycleExpiration>,
		/// Expiration policy for the versions of an object that are not its
		/// current version any more
		pub noncurrent_version_expiration: Option<NoncurrentVersionExpiration>,
	}

	/// Expiration policy for the versions of an object that are not its current
	/// version any more
	///
	/// A version becomes noncurrent when a newer version of the object is
	/// written, and is permanently deleted `noncurrent_days` days after that.
	#[derive(PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct NoncurrentVersionExpiration {
		/// Number of days after a version stopped being the current one after
		/// which it is permanently deleted
		pub noncurrent_days: usize,
		/// If set, that many of the most recent noncurrent versions are kept
		/// whatever their age
		pub newer_noncurrent_versions: Option<usize>,
	}

	/// The versioning state of a bucket
	///
	/// Once versioning has been enabled on a bucket, it can never go back to
	/// `Disabled`: it can only be `Suspended`, which stops the creation of new
	/// versions but keeps the versions that already exist.
	#[derive(
		Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize,
	)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub enum VersioningState {
		/// Versioning was never enabled on this bucket: objects have no
		/// version id and writing an object overwrites the previous one
		#[default]
		Disabled,
		/// Each write creates a new version of the object, and versions are
		/// kept until they are explicitly deleted
		Enabled,
		/// Versioning was enabled and then suspended: versions that were
		/// created while it was enabled are kept, but new writes overwrite
		/// the object's `null` version
		Suspended,
	}

	/// Object Lock configuration of a bucket
	///
	/// Object Lock can only be turned on when the bucket is created, and can
	/// never be turned off afterwards. A bucket that has Object Lock enabled
	/// always has versioning enabled as well.
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct ObjectLockConfig {
		/// Retention settings applied to new object versions when the request
		/// that creates them does not specify any
		pub default_retention: Option<DefaultRetention>,
	}

	/// Default Object Lock retention settings of a bucket
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub struct DefaultRetention {
		pub mode: ObjectLockMode,
		pub duration: RetentionDuration,
	}

	/// How long a new object version is retained under the bucket's default
	/// Object Lock retention settings
	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Serialize, Deserialize)]
	#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
	pub enum RetentionDuration {
		Days(u64),
		Years(u64),
	}

	fn migrate_lifecycle_rule(old: v2::LifecycleRule) -> LifecycleRule {
		LifecycleRule {
			id: old.id,
			enabled: old.enabled,
			filter: old.filter,
			abort_incomplete_mpu_days: old.abort_incomplete_mpu_days,
			expiration: old.expiration,
			// Rules that predate versioning support cannot expire noncurrent
			// versions, as there were none
			noncurrent_version_expiration: None,
		}
	}

	impl garage_util::migrate::Migrate for Bucket {
		const VERSION_MARKER: &'static [u8] = b"G3bkt";

		type Previous = v2::Bucket;

		fn migrate(old: v2::Bucket) -> Bucket {
			Bucket {
				id: old.id,
				state: old.state.map(|x| BucketParams {
					creation_date: x.creation_date,
					authorized_keys: x.authorized_keys,
					aliases: x.aliases,
					local_aliases: x.local_aliases,
					website_config: x.website_config,
					cors_config: x.cors_config,
					lifecycle_config: x.lifecycle_config.map(|lc| {
						lc.map(|rules| rules.into_iter().map(migrate_lifecycle_rule).collect())
					}),
					quotas: x.quotas,
					// Buckets that predate versioning support have never had
					// versioning nor Object Lock enabled
					versioning: crdt::Lww::raw(x.creation_date, VersioningState::Disabled),
					object_lock: crdt::Lww::raw(x.creation_date, None.into()),
				}),
			}
		}
	}
}

pub use v3::*;

impl AutoCrdt for BucketQuotas {
	const WARN_IF_DIFFERENT: bool = true;
}

impl AutoCrdt for VersioningState {
	const WARN_IF_DIFFERENT: bool = true;
}

impl BucketParams {
	/// The versioning state of this bucket
	pub fn versioning(&self) -> VersioningState {
		*self.versioning.get()
	}

	/// Whether new writes to this bucket must create a new retained version
	pub fn is_versioning_enabled(&self) -> bool {
		self.versioning() == VersioningState::Enabled
	}

	/// The Object Lock configuration of this bucket, if Object Lock is enabled
	pub fn object_lock(&self) -> Option<&ObjectLockConfig> {
		self.object_lock.get().inner()
	}
}

impl RetentionDuration {
	/// The number of milliseconds this duration represents.
	///
	/// As in AWS S3, a year is counted as 365 days.
	pub fn as_msec(&self) -> u64 {
		const DAY_MSEC: u64 = 24 * 3600 * 1000;
		match self {
			Self::Days(d) => d.saturating_mul(DAY_MSEC),
			Self::Years(y) => y.saturating_mul(365).saturating_mul(DAY_MSEC),
		}
	}
}

impl BucketParams {
	/// Create an empty `BucketParams` with no authorized keys and no website access
	fn new() -> Self {
		BucketParams {
			creation_date: now_msec(),
			authorized_keys: crdt::Map::new(),
			aliases: crdt::LwwMap::new(),
			local_aliases: crdt::LwwMap::new(),
			website_config: crdt::Lww::new(None.into()),
			cors_config: crdt::Lww::new(None.into()),
			lifecycle_config: crdt::Lww::new(None.into()),
			quotas: crdt::Lww::new(BucketQuotas::default()),
			versioning: crdt::Lww::new(VersioningState::Disabled),
			object_lock: crdt::Lww::new(None.into()),
		}
	}
}

impl Crdt for BucketParams {
	fn merge(&mut self, o: &Self) {
		self.creation_date = std::cmp::min(self.creation_date, o.creation_date);
		self.authorized_keys.merge(&o.authorized_keys);

		self.aliases.merge(&o.aliases);
		self.local_aliases.merge(&o.local_aliases);

		self.website_config.merge(&o.website_config);
		self.cors_config.merge(&o.cors_config);
		self.lifecycle_config.merge(&o.lifecycle_config);
		self.quotas.merge(&o.quotas);
		self.versioning.merge(&o.versioning);
		self.object_lock.merge(&o.object_lock);
	}
}

pub fn parse_lifecycle_date(date: &str) -> Result<chrono::NaiveDate, &'static str> {
	use chrono::prelude::*;

	if let Ok(datetime) = NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%SZ") {
		if datetime.time() == NaiveTime::MIN {
			Ok(datetime.date())
		} else {
			Err("date must be at midnight")
		}
	} else {
		NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "date has invalid format")
	}
}

impl Default for Bucket {
	fn default() -> Self {
		Self::new()
	}
}

impl Default for BucketParams {
	fn default() -> Self {
		Self::new()
	}
}

impl Bucket {
	/// Initializes a new instance of the Bucket struct
	pub fn new() -> Self {
		Bucket {
			id: gen_uuid(),
			state: crdt::Deletable::present(BucketParams::new()),
		}
	}

	pub fn present(id: Uuid, params: BucketParams) -> Self {
		Bucket {
			id,
			state: crdt::Deletable::present(params),
		}
	}

	/// Returns true if this represents a deleted bucket
	pub fn is_deleted(&self) -> bool {
		self.state.is_deleted()
	}

	/// Returns an option representing the parameters (None if in deleted state)
	pub fn params(&self) -> Option<&BucketParams> {
		self.state.as_option()
	}

	/// Mutable version of `.params()`
	pub fn params_mut(&mut self) -> Option<&mut BucketParams> {
		self.state.as_option_mut()
	}

	/// Return the list of authorized keys, when each was updated, and the permission associated to
	/// the key
	pub fn authorized_keys(&self) -> &[(String, BucketKeyPerm)] {
		self.params()
			.map(|s| s.authorized_keys.items())
			.unwrap_or(&[])
	}

	pub fn aliases(&self) -> &[(String, u64, bool)] {
		self.params().map(|s| s.aliases.items()).unwrap_or(&[])
	}

	pub fn local_aliases(&self) -> &[((String, String), u64, bool)] {
		self.params()
			.map(|s| s.local_aliases.items())
			.unwrap_or(&[])
	}
}

impl Entry<EmptyKey, Uuid> for Bucket {
	fn partition_key(&self) -> &EmptyKey {
		&EmptyKey
	}
	fn sort_key(&self) -> &Uuid {
		&self.id
	}
}

impl Crdt for Bucket {
	fn merge(&mut self, other: &Self) {
		self.state.merge(&other.state);
	}
}

pub struct BucketTable;

impl TableSchema for BucketTable {
	const TABLE_NAME: &'static str = "bucket_v2";

	type P = EmptyKey;
	type S = Uuid;
	type E = Bucket;
	type Filter = DeletedFilter;

	fn matches_filter(entry: &Self::E, filter: &Self::Filter) -> bool {
		filter.apply(entry.is_deleted())
	}
}
