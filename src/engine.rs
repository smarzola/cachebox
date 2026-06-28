//! In-memory cache engine.
//!
//! The MVP engine stores byte keys and values with lazy expiration. Networking
//! and request parsing stay outside this module.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cachebox_protocol::Ttl;

const ENTRY_OVERHEAD_BYTES: usize = 96;
const LRU_SAMPLE_SIZE: usize = 16;
pub const DEFAULT_SHARD_COUNT: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

pub trait Clock: Clone {
    fn now_ms(&self) -> u64;
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must not be before unix epoch")
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Debug)]
pub struct Engine<C = SystemClock>
where
    C: Clock,
{
    clock: C,
    entries: HashMap<EntryId, Entry>,
    leases: HashMap<EntryId, Lease>,
    tag_index: HashMap<TagId, HashSet<EntryId>>,
    expiry_index: BTreeSet<ExpiryKey>,
    limits: EngineLimits,
    memory_used_bytes: usize,
    cost_score_total: u64,
    next_access: u64,
    stats: EngineStats,
}

impl Engine<SystemClock> {
    pub fn new() -> Self {
        Self::with_clock_and_limits(SystemClock, EngineLimits::default())
    }

    pub fn with_limits(limits: EngineLimits) -> Self {
        Self::with_clock_and_limits(SystemClock, limits)
    }
}

impl Default for Engine<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ShardedEngine<C = SystemClock>
where
    C: Clock,
{
    shards: Vec<Mutex<Engine<C>>>,
    tag_directory: Mutex<HashMap<TagId, HashSet<usize>>>,
    limits: EngineLimits,
}

impl ShardedEngine<SystemClock> {
    pub fn with_limits(limits: EngineLimits) -> Self {
        Self::with_clock_and_limits(SystemClock, limits, DEFAULT_SHARD_COUNT)
    }
}

impl<C> ShardedEngine<C>
where
    C: Clock,
{
    pub fn with_clock_and_limits(clock: C, limits: EngineLimits, shard_count: usize) -> Self {
        assert!(shard_count > 0, "sharded engine needs at least one shard");
        let shard_limits = EngineLimits {
            max_memory_bytes: limits.max_memory_bytes.div_ceil(shard_count),
            max_value_bytes: limits.max_value_bytes,
        };
        let shards = (0..shard_count)
            .map(|_| Mutex::new(Engine::with_clock_and_limits(clock.clone(), shard_limits)))
            .collect();
        Self {
            shards,
            tag_directory: Mutex::new(HashMap::new()),
            limits,
        }
    }

    pub fn put(&self, command: PutCommand) -> Result<PutOutcome, PutError> {
        let namespace = command.namespace.clone();
        let key = command.key.clone();
        let new_tags = tag_ids(&namespace, &command.tags);
        let shard_index = self.shard_index_for(&namespace, &key);
        let mut shard = self.shards[shard_index]
            .lock()
            .expect("engine shard mutex poisoned");
        let old_tags = shard.tag_ids_for_entry(&namespace, &key);
        let result = shard.put(command);
        let old_tags_without_entries = old_tags
            .into_iter()
            .filter(|tag_id| !shard.has_tag_id(tag_id))
            .collect::<Vec<_>>();
        drop(shard);

        let add_tags = if result.is_ok() { new_tags } else { Vec::new() };
        self.update_tag_routes(shard_index, add_tags, old_tags_without_entries);
        result
    }

    pub fn get(&self, namespace: &str, key: &[u8]) -> GetOutcome {
        self.shard_for(namespace, key)
            .lock()
            .expect("engine shard mutex poisoned")
            .get(namespace, key)
    }

    pub fn get_ref<R>(
        &self,
        namespace: &str,
        key: &[u8],
        map: impl FnOnce(GetOutcomeRef<'_>) -> R,
    ) -> R {
        self.shard_for(namespace, key)
            .lock()
            .expect("engine shard mutex poisoned")
            .get_ref(namespace, key, map)
    }

    pub fn get_ref_without_access_update<R>(
        &self,
        namespace: &str,
        key: &[u8],
        map: impl FnOnce(GetOutcomeRef<'_>) -> R,
    ) -> R {
        self.shard_for(namespace, key)
            .lock()
            .expect("engine shard mutex poisoned")
            .get_ref_without_access_update(namespace, key, map)
    }

    pub fn delete(&self, namespace: &str, key: &[u8]) -> bool {
        let shard_index = self.shard_index_for(namespace, key);
        let mut shard = self.shards[shard_index]
            .lock()
            .expect("engine shard mutex poisoned");
        let old_tags = shard.tag_ids_for_entry(namespace, key);
        let removed = shard.delete(namespace, key);
        let old_tags_without_entries = old_tags
            .into_iter()
            .filter(|tag_id| !shard.has_tag_id(tag_id))
            .collect::<Vec<_>>();
        drop(shard);

        if removed {
            self.update_tag_routes(shard_index, Vec::new(), old_tags_without_entries);
        }
        removed
    }

    pub fn batch_get(&self, namespace: &str, keys: &[Vec<u8>]) -> Vec<GetOutcome> {
        keys.iter().map(|key| self.get(namespace, key)).collect()
    }

    pub fn invalidate_tag(&self, namespace: &str, tag: &str) -> usize {
        let tag_id = TagId {
            namespace: namespace.to_string(),
            tag: tag.to_string(),
        };
        let shard_indices = self
            .tag_directory
            .lock()
            .expect("tag directory mutex poisoned")
            .remove(&tag_id)
            .map(|indices| indices.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();

        shard_indices
            .into_iter()
            .map(|index| {
                self.shards[index]
                    .lock()
                    .expect("engine shard mutex poisoned")
                    .invalidate_tag(namespace, tag)
            })
            .sum()
    }

    pub fn start_lease(&self, namespace: &str, key: &[u8], lease_ttl_ms: u64) -> StartLeaseOutcome {
        self.shard_for(namespace, key)
            .lock()
            .expect("engine shard mutex poisoned")
            .start_lease(namespace, key, lease_ttl_ms)
    }

    pub fn complete_lease(
        &self,
        command: CompleteLeaseCommand,
    ) -> Result<PutOutcome, CompleteLeaseError> {
        let namespace = command.namespace.clone();
        let key = command.key.clone();
        let new_tags = tag_ids(&namespace, &command.tags);
        let shard_index = self.shard_index_for(&namespace, &key);
        let mut shard = self.shards[shard_index]
            .lock()
            .expect("engine shard mutex poisoned");
        let old_tags = shard.tag_ids_for_entry(&namespace, &key);
        let result = shard.complete_lease(command);
        let old_tags_without_entries = old_tags
            .into_iter()
            .filter(|tag_id| !shard.has_tag_id(tag_id))
            .collect::<Vec<_>>();
        drop(shard);

        let add_tags = if result.is_ok() { new_tags } else { Vec::new() };
        self.update_tag_routes(shard_index, add_tags, old_tags_without_entries);
        result
    }

    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.lock().expect("engine shard mutex poisoned").len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn memory_used_bytes(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| {
                shard
                    .lock()
                    .expect("engine shard mutex poisoned")
                    .memory_used_bytes()
            })
            .sum()
    }

    pub fn cost_score_total(&self) -> u64 {
        self.shards
            .iter()
            .map(|shard| {
                shard
                    .lock()
                    .expect("engine shard mutex poisoned")
                    .cost_score_total()
            })
            .sum()
    }

    pub fn limits(&self) -> EngineLimits {
        self.limits
    }

    pub fn stats(&self) -> EngineStats {
        self.shards
            .iter()
            .map(|shard| shard.lock().expect("engine shard mutex poisoned").stats())
            .fold(EngineStats::default(), |mut total, stats| {
                total.expirations = total.expirations.saturating_add(stats.expirations);
                total.evictions = total.evictions.saturating_add(stats.evictions);
                total
            })
    }

    pub fn reclaim_expired_budget(&self, max_entries: usize) -> usize {
        if max_entries == 0 {
            return 0;
        }
        let mut remaining = max_entries;
        let mut removed = 0;
        for shard in &self.shards {
            if remaining == 0 {
                break;
            }
            let shard_removed = shard
                .lock()
                .expect("engine shard mutex poisoned")
                .reclaim_expired_budget(remaining);
            removed += shard_removed;
            remaining = remaining.saturating_sub(shard_removed);
        }
        removed
    }

    fn shard_for(&self, namespace: &str, key: &[u8]) -> &Mutex<Engine<C>> {
        &self.shards[self.shard_index_for(namespace, key)]
    }

    fn shard_index_for(&self, namespace: &str, key: &[u8]) -> usize {
        let mut hasher = DefaultHasher::new();
        namespace.hash(&mut hasher);
        key.hash(&mut hasher);
        (hasher.finish() as usize) % self.shards.len()
    }

    fn update_tag_routes(&self, shard_index: usize, add_tags: Vec<TagId>, remove_tags: Vec<TagId>) {
        if add_tags.is_empty() && remove_tags.is_empty() {
            return;
        }
        let mut directory = self
            .tag_directory
            .lock()
            .expect("tag directory mutex poisoned");
        for tag_id in remove_tags {
            if let Some(indices) = directory.get_mut(&tag_id) {
                indices.remove(&shard_index);
                if indices.is_empty() {
                    directory.remove(&tag_id);
                }
            }
        }
        for tag_id in add_tags {
            directory.entry(tag_id).or_default().insert(shard_index);
        }
    }

    #[cfg(test)]
    fn routed_shard_count_for_tag(&self, namespace: &str, tag: &str) -> usize {
        let tag_id = TagId {
            namespace: namespace.to_string(),
            tag: tag.to_string(),
        };
        self.tag_directory
            .lock()
            .expect("tag directory mutex poisoned")
            .get(&tag_id)
            .map(HashSet::len)
            .unwrap_or(0)
    }
}

impl<C> Engine<C>
where
    C: Clock,
{
    pub fn with_clock(clock: C) -> Self {
        Self::with_clock_and_limits(clock, EngineLimits::default())
    }

    pub fn with_clock_and_limits(clock: C, limits: EngineLimits) -> Self {
        Self {
            clock,
            entries: HashMap::new(),
            leases: HashMap::new(),
            tag_index: HashMap::new(),
            expiry_index: BTreeSet::new(),
            limits,
            memory_used_bytes: 0,
            cost_score_total: 0,
            next_access: 0,
            stats: EngineStats::default(),
        }
    }

    pub fn put(&mut self, command: PutCommand) -> Result<PutOutcome, PutError> {
        if command.value.len() > self.limits.max_value_bytes {
            return Err(PutError::ValueTooLarge {
                value_bytes: command.value.len(),
                max_value_bytes: self.limits.max_value_bytes,
            });
        }

        let now_ms = self.clock.now_ms();
        let id = EntryId {
            namespace: command.namespace,
            key: command.key,
        };
        let entry_memory_bytes = estimate_entry_memory(&id, &command.value, &command.tags);
        if entry_memory_bytes > self.limits.max_memory_bytes {
            return Err(PutError::ValueTooLargeForMemory {
                entry_bytes: entry_memory_bytes,
                max_memory_bytes: self.limits.max_memory_bytes,
            });
        }

        self.remove_entry(&id);
        if self.memory_used_bytes.saturating_add(entry_memory_bytes) > self.limits.max_memory_bytes
        {
            self.reclaim_expired();
        }
        let evicted = self.evict_until_fits(entry_memory_bytes);
        if self.memory_used_bytes.saturating_add(entry_memory_bytes) > self.limits.max_memory_bytes
        {
            return Err(PutError::InsufficientMemory {
                required_bytes: entry_memory_bytes,
                memory_used_bytes: self.memory_used_bytes,
                max_memory_bytes: self.limits.max_memory_bytes,
            });
        }

        let expires_at_ms = command
            .ttl
            .map(|ttl| now_ms.saturating_add(ttl.milliseconds));
        let stale_until_ms = match (expires_at_ms, command.stale_ttl) {
            (Some(expires_at), Some(stale_ttl)) => {
                Some(expires_at.saturating_add(stale_ttl.milliseconds))
            }
            _ => None,
        };

        let tags = command.tags;
        for tag in &tags {
            self.tag_index
                .entry(TagId {
                    namespace: id.namespace.clone(),
                    tag: tag.clone(),
                })
                .or_default()
                .insert(id.clone());
        }

        let last_access = self.bump_access();
        if let Some(removable_at_ms) = removable_at_ms(expires_at_ms, stale_until_ms) {
            self.expiry_index.insert(ExpiryKey {
                expires_at_ms: removable_at_ms,
                id: id.clone(),
            });
        }
        self.entries.insert(
            id,
            Entry {
                value: command.value,
                expires_at_ms,
                stale_until_ms,
                tags,
                cost: command.cost,
                memory_bytes: entry_memory_bytes,
                last_access,
            },
        );
        self.memory_used_bytes = self.memory_used_bytes.saturating_add(entry_memory_bytes);
        self.cost_score_total = self
            .cost_score_total
            .saturating_add(command.cost.unwrap_or(0));
        Ok(PutOutcome { evicted })
    }

    pub fn get(&mut self, namespace: &str, key: &[u8]) -> GetOutcome {
        self.get_ref(namespace, key, |outcome| match outcome {
            GetOutcomeRef::Hit(value) => GetOutcome::Hit(value.to_vec()),
            GetOutcomeRef::Stale(value) => GetOutcome::Stale(value.to_vec()),
            GetOutcomeRef::Miss => GetOutcome::Miss,
        })
    }

    pub fn get_ref<R>(
        &mut self,
        namespace: &str,
        key: &[u8],
        map: impl FnOnce(GetOutcomeRef<'_>) -> R,
    ) -> R {
        self.get_ref_inner(namespace, key, true, map)
    }

    pub fn get_ref_without_access_update<R>(
        &mut self,
        namespace: &str,
        key: &[u8],
        map: impl FnOnce(GetOutcomeRef<'_>) -> R,
    ) -> R {
        self.get_ref_inner(namespace, key, false, map)
    }

    fn get_ref_inner<R>(
        &mut self,
        namespace: &str,
        key: &[u8],
        update_access: bool,
        map: impl FnOnce(GetOutcomeRef<'_>) -> R,
    ) -> R {
        let id = EntryId::new(namespace, key);
        let now_ms = self.clock.now_ms();
        let Some(entry) = self.entries.get_mut(&id) else {
            return map(GetOutcomeRef::Miss);
        };
        match entry_state_at(entry, now_ms) {
            EntryState::Missing => map(GetOutcomeRef::Miss),
            EntryState::Expired => {
                self.remove_entry(&id);
                map(GetOutcomeRef::Miss)
            }
            EntryState::Fresh => {
                if update_access {
                    let access = self.next_access;
                    self.next_access = self.next_access.saturating_add(1);
                    entry.last_access = access;
                }
                map(GetOutcomeRef::Hit(&entry.value))
            }
            EntryState::Stale => {
                if update_access {
                    let access = self.next_access;
                    self.next_access = self.next_access.saturating_add(1);
                    entry.last_access = access;
                }
                map(GetOutcomeRef::Stale(&entry.value))
            }
        }
    }

    pub fn delete(&mut self, namespace: &str, key: &[u8]) -> bool {
        let id = EntryId::new(namespace, key);
        self.leases.remove(&id);
        self.remove_entry(&id)
    }

    pub fn batch_get(&mut self, namespace: &str, keys: &[Vec<u8>]) -> Vec<GetOutcome> {
        keys.iter().map(|key| self.get(namespace, key)).collect()
    }

    pub fn invalidate_tag(&mut self, namespace: &str, tag: &str) -> usize {
        let tag_id = TagId {
            namespace: namespace.to_string(),
            tag: tag.to_string(),
        };
        let Some(ids) = self.tag_index.remove(&tag_id) else {
            return 0;
        };

        let mut removed = 0;
        for id in ids {
            if self.remove_entry(&id) {
                removed += 1;
            }
        }
        removed
    }

    fn tag_ids_for_entry(&self, namespace: &str, key: &[u8]) -> Vec<TagId> {
        let id = EntryId::new(namespace, key);
        self.entries
            .get(&id)
            .map(|entry| tag_ids(namespace, &entry.tags))
            .unwrap_or_default()
    }

    fn has_tag_id(&self, tag_id: &TagId) -> bool {
        self.tag_index
            .get(tag_id)
            .is_some_and(|ids| !ids.is_empty())
    }

    pub fn start_lease(
        &mut self,
        namespace: &str,
        key: &[u8],
        lease_ttl_ms: u64,
    ) -> StartLeaseOutcome {
        let id = EntryId::new(namespace, key);
        let has_active_lease = self.active_lease(&id).is_some();
        match self.entry_state(&id) {
            EntryState::Fresh => {
                self.record_access(&id);
                if let Some(entry) = self.entries.get(&id) {
                    StartLeaseOutcome::Hit(entry.value.clone())
                } else {
                    StartLeaseOutcome::LeaseGranted {
                        token: self.create_lease(&id, lease_ttl_ms),
                        stale_value: None,
                    }
                }
            }
            EntryState::Stale => {
                self.record_access(&id);
                let stale_value = self.entries.get(&id).map(|entry| entry.value.clone());
                if has_active_lease {
                    StartLeaseOutcome::Stale {
                        value: stale_value.unwrap_or_default(),
                    }
                } else {
                    StartLeaseOutcome::LeaseGranted {
                        token: self.create_lease(&id, lease_ttl_ms),
                        stale_value,
                    }
                }
            }
            EntryState::Expired => {
                if has_active_lease {
                    return StartLeaseOutcome::LeaseDenied;
                }
                self.remove_entry(&id);
                self.start_lease_for_missing(id, lease_ttl_ms)
            }
            EntryState::Missing => self.start_lease_for_missing(id, lease_ttl_ms),
        }
    }

    pub fn complete_lease(
        &mut self,
        command: CompleteLeaseCommand,
    ) -> Result<PutOutcome, CompleteLeaseError> {
        let id = EntryId {
            namespace: command.namespace.clone(),
            key: command.key.clone(),
        };
        let Some(lease) = self.active_lease(&id) else {
            self.leases.remove(&id);
            return Err(CompleteLeaseError::InvalidLeaseToken);
        };
        if lease.token != command.lease_token {
            return Err(CompleteLeaseError::InvalidLeaseToken);
        }
        self.leases.remove(&id);
        self.put(PutCommand {
            namespace: command.namespace,
            key: command.key,
            value: command.value,
            ttl: command.ttl,
            stale_ttl: command.stale_ttl,
            tags: command.tags,
            cost: command.cost,
        })
        .map_err(CompleteLeaseError::Put)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn memory_used_bytes(&self) -> usize {
        self.memory_used_bytes
    }

    pub fn cost_score_total(&self) -> u64 {
        self.cost_score_total
    }

    pub fn limits(&self) -> EngineLimits {
        self.limits
    }

    pub fn stats(&self) -> EngineStats {
        self.stats
    }

    pub fn reclaim_expired(&mut self) -> usize {
        self.reclaim_expired_budget(usize::MAX)
    }

    pub fn reclaim_expired_budget(&mut self, max_entries: usize) -> usize {
        self.remove_expired(max_entries)
    }

    fn entry_state(&self, id: &EntryId) -> EntryState {
        let Some(entry) = self.entries.get(id) else {
            return EntryState::Missing;
        };
        let now_ms = self.clock.now_ms();
        entry_state_at(entry, now_ms)
    }

    fn remove_expired(&mut self, max_entries: usize) -> usize {
        let now_ms = self.clock.now_ms();
        let mut expired = Vec::new();
        while expired.len() < max_entries {
            let Some(key) = self.expiry_index.first().cloned() else {
                break;
            };
            if key.expires_at_ms > now_ms {
                break;
            }
            self.expiry_index.remove(&key);
            expired.push(key.id);
        }

        let mut removed = 0;
        for id in expired {
            if self.remove_entry(&id) {
                self.stats.expirations += 1;
                removed += 1;
            }
        }
        removed
    }

    fn remove_entry(&mut self, id: &EntryId) -> bool {
        let Some(entry) = self.entries.remove(id) else {
            return false;
        };
        self.leases.remove(id);
        self.memory_used_bytes = self.memory_used_bytes.saturating_sub(entry.memory_bytes);
        self.cost_score_total = self
            .cost_score_total
            .saturating_sub(entry.cost.unwrap_or(0));
        if let Some(removable_at_ms) = removable_at_ms(entry.expires_at_ms, entry.stale_until_ms) {
            self.expiry_index.remove(&ExpiryKey {
                expires_at_ms: removable_at_ms,
                id: id.clone(),
            });
        }

        for tag in entry.tags {
            let tag_id = TagId {
                namespace: id.namespace.clone(),
                tag,
            };
            if let Some(ids) = self.tag_index.get_mut(&tag_id) {
                ids.remove(id);
                if ids.is_empty() {
                    self.tag_index.remove(&tag_id);
                }
            }
        }

        true
    }

    fn evict_until_fits(&mut self, incoming_bytes: usize) -> usize {
        let mut evicted = 0;
        while self.memory_used_bytes.saturating_add(incoming_bytes) > self.limits.max_memory_bytes {
            let Some(id) = self.least_recently_used_entry() else {
                break;
            };
            if self.remove_entry(&id) {
                evicted += 1;
                self.stats.evictions += 1;
            } else {
                break;
            }
        }
        evicted
    }

    fn least_recently_used_entry(&self) -> Option<EntryId> {
        self.entries
            .iter()
            .take(LRU_SAMPLE_SIZE)
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(id, _)| id.clone())
    }

    fn record_access(&mut self, id: &EntryId) {
        if !self.entries.contains_key(id) {
            return;
        };
        let access = self.bump_access();
        if let Some(entry) = self.entries.get_mut(id) {
            entry.last_access = access;
        }
    }

    fn bump_access(&mut self) -> u64 {
        let access = self.next_access;
        self.next_access = self.next_access.saturating_add(1);
        access
    }

    fn start_lease_for_missing(&mut self, id: EntryId, lease_ttl_ms: u64) -> StartLeaseOutcome {
        if self.active_lease(&id).is_some() {
            StartLeaseOutcome::LeaseDenied
        } else {
            StartLeaseOutcome::LeaseGranted {
                token: self.create_lease(&id, lease_ttl_ms),
                stale_value: None,
            }
        }
    }

    fn active_lease(&self, id: &EntryId) -> Option<&Lease> {
        let lease = self.leases.get(id)?;
        if self.clock.now_ms() <= lease.expires_at_ms {
            Some(lease)
        } else {
            None
        }
    }

    fn create_lease(&mut self, id: &EntryId, lease_ttl_ms: u64) -> String {
        let token = format!("lease-{}", self.next_access);
        self.next_access = self.next_access.saturating_add(1);
        self.leases.insert(
            id.clone(),
            Lease {
                token: token.clone(),
                expires_at_ms: self.clock.now_ms().saturating_add(lease_ttl_ms),
            },
        );
        token
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineLimits {
    pub max_memory_bytes: usize,
    pub max_value_bytes: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_value_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    pub expirations: u64,
    pub evictions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutOutcome {
    pub evicted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutError {
    ValueTooLarge {
        value_bytes: usize,
        max_value_bytes: usize,
    },
    ValueTooLargeForMemory {
        entry_bytes: usize,
        max_memory_bytes: usize,
    },
    InsufficientMemory {
        required_bytes: usize,
        memory_used_bytes: usize,
        max_memory_bytes: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteLeaseCommand {
    pub namespace: String,
    pub key: Vec<u8>,
    pub lease_token: String,
    pub value: Vec<u8>,
    pub ttl: Option<Ttl>,
    pub stale_ttl: Option<Ttl>,
    pub tags: Vec<String>,
    pub cost: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteLeaseError {
    InvalidLeaseToken,
    Put(PutError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartLeaseOutcome {
    Hit(Vec<u8>),
    Stale {
        value: Vec<u8>,
    },
    LeaseGranted {
        token: String,
        stale_value: Option<Vec<u8>>,
    },
    LeaseDenied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutCommand {
    pub namespace: String,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub ttl: Option<Ttl>,
    pub stale_ttl: Option<Ttl>,
    pub tags: Vec<String>,
    pub cost: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetOutcome {
    Hit(Vec<u8>),
    Stale(Vec<u8>),
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetOutcomeRef<'a> {
    Hit(&'a [u8]),
    Stale(&'a [u8]),
    Miss,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct EntryId {
    namespace: String,
    key: Vec<u8>,
}

impl EntryId {
    fn new(namespace: &str, key: &[u8]) -> Self {
        Self {
            namespace: namespace.to_string(),
            key: key.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExpiryKey {
    expires_at_ms: u64,
    id: EntryId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TagId {
    namespace: String,
    tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    value: Vec<u8>,
    expires_at_ms: Option<u64>,
    stale_until_ms: Option<u64>,
    tags: Vec<String>,
    cost: Option<u64>,
    memory_bytes: usize,
    last_access: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Lease {
    token: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    Missing,
    Fresh,
    Stale,
    Expired,
}

fn estimate_entry_memory(id: &EntryId, value: &[u8], tags: &[String]) -> usize {
    ENTRY_OVERHEAD_BYTES
        .saturating_add(id.namespace.len())
        .saturating_add(id.key.len())
        .saturating_add(value.len())
        .saturating_add(tags.iter().map(String::len).sum::<usize>())
}

fn tag_ids(namespace: &str, tags: &[String]) -> Vec<TagId> {
    tags.iter()
        .map(|tag| TagId {
            namespace: namespace.to_string(),
            tag: tag.clone(),
        })
        .collect()
}

fn removable_at_ms(expires_at_ms: Option<u64>, stale_until_ms: Option<u64>) -> Option<u64> {
    stale_until_ms.or(expires_at_ms)
}

fn entry_state_at(entry: &Entry, now_ms: u64) -> EntryState {
    match (entry.expires_at_ms, entry.stale_until_ms) {
        (None, _) => EntryState::Fresh,
        (Some(expires_at), _) if now_ms <= expires_at => EntryState::Fresh,
        (Some(_), Some(stale_until)) if now_ms <= stale_until => EntryState::Stale,
        _ => EntryState::Expired,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    use super::*;

    #[derive(Clone, Debug)]
    struct ManualClock {
        now_ms: Rc<Cell<u64>>,
    }

    impl ManualClock {
        fn new(now_ms: u64) -> Self {
            Self {
                now_ms: Rc::new(Cell::new(now_ms)),
            }
        }

        fn advance(&self, milliseconds: u64) {
            self.now_ms
                .set(self.now_ms.get().saturating_add(milliseconds));
        }
    }

    impl Clock for ManualClock {
        fn now_ms(&self) -> u64 {
            self.now_ms.get()
        }
    }

    fn engine() -> (Engine<ManualClock>, ManualClock) {
        let clock = ManualClock::new(1_000);
        (Engine::with_clock(clock.clone()), clock)
    }

    fn tiny_engine(
        max_memory_bytes: usize,
        max_value_bytes: usize,
    ) -> (Engine<ManualClock>, ManualClock) {
        let clock = ManualClock::new(1_000);
        (
            Engine::with_clock_and_limits(
                clock.clone(),
                EngineLimits {
                    max_memory_bytes,
                    max_value_bytes,
                },
            ),
            clock,
        )
    }

    fn sharded_engine(shard_count: usize) -> (ShardedEngine<ManualClock>, ManualClock) {
        let clock = ManualClock::new(1_000);
        (
            ShardedEngine::with_clock_and_limits(
                clock.clone(),
                EngineLimits::default(),
                shard_count,
            ),
            clock,
        )
    }

    fn put(namespace: &str, key: &[u8], value: &[u8]) -> PutCommand {
        PutCommand {
            namespace: namespace.to_string(),
            key: key.to_vec(),
            value: value.to_vec(),
            ttl: None,
            stale_ttl: None,
            tags: Vec::new(),
            cost: None,
        }
    }

    #[test]
    fn sharded_engine_invalidates_tag_across_shards() {
        let (engine, _) = sharded_engine(8);
        for index in 0..64 {
            let mut command = put("default", format!("key-{index}").as_bytes(), b"value");
            command.tags = vec!["group".to_string()];
            engine.put(command).expect("put should fit");
        }

        assert_eq!(engine.len(), 64);
        assert!(engine.routed_shard_count_for_tag("default", "group") > 1);
        assert!(engine.routed_shard_count_for_tag("default", "group") <= 8);
        assert_eq!(engine.invalidate_tag("default", "group"), 64);
        assert_eq!(engine.routed_shard_count_for_tag("default", "group"), 0);
        assert_eq!(engine.len(), 0);
        for index in 0..64 {
            assert_eq!(
                engine.get("default", format!("key-{index}").as_bytes()),
                GetOutcome::Miss
            );
        }
    }

    #[test]
    fn sharded_tag_routing_cleans_replaced_tags() {
        let (engine, _) = sharded_engine(8);
        let mut first = put("default", b"k", b"old");
        first.tags = vec!["old".to_string()];
        engine.put(first).expect("put should fit");
        assert_eq!(engine.routed_shard_count_for_tag("default", "old"), 1);

        let mut second = put("default", b"k", b"new");
        second.tags = vec!["new".to_string()];
        engine.put(second).expect("put should fit");

        assert_eq!(engine.routed_shard_count_for_tag("default", "old"), 0);
        assert_eq!(engine.invalidate_tag("default", "old"), 0);
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Hit(b"new".to_vec())
        );
        assert_eq!(engine.routed_shard_count_for_tag("default", "new"), 1);
        assert_eq!(engine.invalidate_tag("default", "new"), 1);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn sharded_tag_routing_cleans_deleted_entries() {
        let (engine, _) = sharded_engine(8);
        let mut command = put("default", b"k", b"value");
        command.tags = vec!["group".to_string()];
        engine.put(command).expect("put should fit");
        assert_eq!(engine.routed_shard_count_for_tag("default", "group"), 1);

        assert!(engine.delete("default", b"k"));

        assert_eq!(engine.routed_shard_count_for_tag("default", "group"), 0);
        assert_eq!(engine.invalidate_tag("default", "group"), 0);
    }

    #[test]
    fn sharded_tag_routing_preserves_namespace_isolation() {
        let (engine, _) = sharded_engine(8);
        let mut first = put("default", b"a", b"1");
        first.tags = vec!["org:9".to_string()];
        engine.put(first).expect("put should fit");

        let mut second = put("other", b"a", b"2");
        second.tags = vec!["org:9".to_string()];
        engine.put(second).expect("put should fit");

        assert_eq!(engine.invalidate_tag("default", "org:9"), 1);
        assert_eq!(engine.get("default", b"a"), GetOutcome::Miss);
        assert_eq!(engine.get("other", b"a"), GetOutcome::Hit(b"2".to_vec()));
        assert_eq!(engine.invalidate_tag("other", "org:9"), 1);
    }

    #[test]
    fn sharded_engine_reclaim_expired_budget_is_global() {
        let (engine, clock) = sharded_engine(8);
        for index in 0..16 {
            let mut command = put("default", format!("key-{index}").as_bytes(), b"value");
            command.ttl = Some(Ttl { milliseconds: 1 });
            engine.put(command).expect("put should fit");
        }

        clock.advance(2);

        assert_eq!(engine.reclaim_expired_budget(5), 5);
        assert_eq!(engine.stats().expirations, 5);
        assert_eq!(engine.len(), 11);
        assert_eq!(engine.reclaim_expired_budget(usize::MAX), 11);
        assert_eq!(engine.stats().expirations, 16);
        assert_eq!(engine.len(), 0);
    }

    #[test]
    fn sharded_engine_aggregates_memory_and_cost_metrics() {
        let (engine, _) = sharded_engine(8);
        let mut first = put("default", b"a", b"value");
        first.cost = Some(10);
        engine.put(first).expect("put should fit");
        let mut second = put("default", b"b", b"value");
        second.cost = Some(7);
        engine.put(second).expect("put should fit");

        assert!(engine.memory_used_bytes() > 0);
        assert_eq!(engine.cost_score_total(), 17);
        assert_eq!(engine.limits(), EngineLimits::default());
    }

    #[test]
    fn stores_and_reads_binary_keys_and_values() {
        let (mut engine, _) = engine();
        engine
            .put(put("default", b"user\0\xff", b"value\0\xff"))
            .expect("put should fit");

        assert_eq!(
            engine.get("default", b"user\0\xff"),
            GetOutcome::Hit(b"value\0\xff".to_vec())
        );
        assert_eq!(engine.get("other", b"user\0\xff"), GetOutcome::Miss);
    }

    #[test]
    fn get_ref_exposes_value_without_changing_owned_get_behavior() {
        let (mut engine, _) = engine();
        engine
            .put(put("default", b"k", b"value"))
            .expect("put should fit");

        let observed = engine.get_ref("default", b"k", |outcome| match outcome {
            GetOutcomeRef::Hit(value) => value.to_vec(),
            other => panic!("expected hit, got {other:?}"),
        });

        assert_eq!(observed, b"value".to_vec());
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Hit(b"value".to_vec())
        );
    }

    #[test]
    fn deletes_are_idempotent() {
        let (mut engine, _) = engine();
        engine
            .put(put("default", b"k", b"v"))
            .expect("put should fit");

        assert!(engine.delete("default", b"k"));
        assert!(!engine.delete("default", b"k"));
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn batch_get_handles_mixed_hits_and_misses() {
        let (mut engine, _) = engine();
        engine
            .put(put("default", b"a", b"1"))
            .expect("put should fit");
        engine
            .put(put("default", b"c", b"3"))
            .expect("put should fit");

        let outcomes = engine.batch_get("default", &[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);

        assert_eq!(
            outcomes,
            vec![
                GetOutcome::Hit(b"1".to_vec()),
                GetOutcome::Miss,
                GetOutcome::Hit(b"3".to_vec())
            ]
        );
    }

    #[test]
    fn expires_entries_lazily_after_ttl() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"v");
        command.ttl = Some(Ttl { milliseconds: 10 });
        engine.put(command).expect("put should fit");

        assert_eq!(engine.get("default", b"k"), GetOutcome::Hit(b"v".to_vec()));
        clock.advance(10);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Hit(b"v".to_vec()));
        clock.advance(1);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
        assert!(engine.is_empty());
    }

    #[test]
    fn serves_stale_until_stale_ttl_expires() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"v");
        command.ttl = Some(Ttl { milliseconds: 10 });
        command.stale_ttl = Some(Ttl { milliseconds: 20 });
        engine.put(command).expect("put should fit");

        clock.advance(11);
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Stale(b"v".to_vec())
        );

        clock.advance(19);
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Stale(b"v".to_vec())
        );

        clock.advance(1);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn ttl_without_stale_expires_directly_to_miss() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"v");
        command.ttl = Some(Ttl { milliseconds: 1 });
        engine.put(command).expect("put should fit");

        clock.advance(2);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn no_expiry_entry_ignores_stale_ttl() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"v");
        command.stale_ttl = Some(Ttl { milliseconds: 1 });
        engine.put(command).expect("put should fit");

        clock.advance(10_000);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Hit(b"v".to_vec()));
    }

    #[test]
    fn replacing_key_cleans_old_tag_indexes() {
        let (mut engine, _) = engine();
        let mut first = put("default", b"k", b"old");
        first.tags = vec!["old".to_string()];
        engine.put(first).expect("put should fit");

        let mut second = put("default", b"k", b"new");
        second.tags = vec!["new".to_string()];
        engine.put(second).expect("put should fit");

        assert_eq!(engine.invalidate_tag("default", "old"), 0);
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Hit(b"new".to_vec())
        );
        assert_eq!(engine.invalidate_tag("default", "new"), 1);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn tag_invalidation_removes_only_matching_namespace_entries() {
        let (mut engine, _) = engine();
        let mut first = put("default", b"a", b"1");
        first.tags = vec!["org:9".to_string()];
        engine.put(first).expect("put should fit");

        let mut second = put("default", b"b", b"2");
        second.tags = vec!["org:9".to_string()];
        engine.put(second).expect("put should fit");

        let mut other_namespace = put("other", b"a", b"3");
        other_namespace.tags = vec!["org:9".to_string()];
        engine.put(other_namespace).expect("put should fit");

        assert_eq!(engine.invalidate_tag("default", "org:9"), 2);
        assert_eq!(engine.get("default", b"a"), GetOutcome::Miss);
        assert_eq!(engine.get("default", b"b"), GetOutcome::Miss);
        assert_eq!(engine.get("other", b"a"), GetOutcome::Hit(b"3".to_vec()));
    }

    #[test]
    fn rejects_single_values_over_max_value_size() {
        let (mut engine, _) = tiny_engine(1_000, 3);

        let error = engine
            .put(put("default", b"k", b"four"))
            .expect_err("oversized value should fail");

        assert_eq!(
            error,
            PutError::ValueTooLarge {
                value_bytes: 4,
                max_value_bytes: 3
            }
        );
        assert_eq!(engine.get("default", b"k"), GetOutcome::Miss);
    }

    #[test]
    fn rejects_entries_that_cannot_fit_memory_cap() {
        let (mut engine, _) = tiny_engine(10, 1_000);

        let error = engine
            .put(put("default", b"k", b"v"))
            .expect_err("entry should exceed tiny memory cap");

        assert!(matches!(
            error,
            PutError::ValueTooLargeForMemory {
                entry_bytes: _,
                max_memory_bytes: 10
            }
        ));
    }

    #[test]
    fn evicts_least_recently_used_entries_to_stay_under_memory_cap() {
        let (mut engine, _) = tiny_engine(240, 1_000);
        engine
            .put(put("default", b"a", b"1111111111"))
            .expect("a should fit");
        engine
            .put(put("default", b"b", b"2222222222"))
            .expect("b should fit");
        assert_eq!(
            engine.get("default", b"a"),
            GetOutcome::Hit(b"1111111111".to_vec())
        );

        let outcome = engine
            .put(put("default", b"c", b"3333333333"))
            .expect("c should fit by evicting one entry");

        assert_eq!(outcome.evicted, 1);
        assert_eq!(engine.stats().evictions, 1);
        assert_eq!(
            engine.get("default", b"a"),
            GetOutcome::Hit(b"1111111111".to_vec())
        );
        assert_eq!(engine.get("default", b"b"), GetOutcome::Miss);
        assert_eq!(
            engine.get("default", b"c"),
            GetOutcome::Hit(b"3333333333".to_vec())
        );
        assert!(engine.memory_used_bytes() <= engine.limits().max_memory_bytes);
    }

    #[test]
    fn get_ref_without_access_update_does_not_refresh_lru_state() {
        let (mut engine, _) = tiny_engine(240, 1_000);
        engine
            .put(put("default", b"a", b"1111111111"))
            .expect("a should fit");
        engine
            .put(put("default", b"b", b"2222222222"))
            .expect("b should fit");
        engine.get_ref_without_access_update("default", b"a", |outcome| {
            assert!(matches!(outcome, GetOutcomeRef::Hit(b"1111111111")));
        });

        let outcome = engine
            .put(put("default", b"c", b"3333333333"))
            .expect("c should fit by evicting one entry");

        assert_eq!(outcome.evicted, 1);
        assert_eq!(engine.get("default", b"a"), GetOutcome::Miss);
        assert_eq!(
            engine.get("default", b"b"),
            GetOutcome::Hit(b"2222222222".to_vec())
        );
    }

    #[test]
    fn replacing_key_updates_memory_accounting() {
        let (mut engine, _) = tiny_engine(1_000, 1_000);
        engine
            .put(put("default", b"k", b"large-value"))
            .expect("large value should fit");
        let after_large = engine.memory_used_bytes();

        engine
            .put(put("default", b"k", b"s"))
            .expect("smaller replacement should fit");
        let after_small = engine.memory_used_bytes();

        assert!(after_small < after_large);
        assert_eq!(engine.get("default", b"k"), GetOutcome::Hit(b"s".to_vec()));
    }

    #[test]
    fn replacing_key_cleans_old_expiry_index() {
        let (mut engine, clock) = engine();
        let mut expiring = put("default", b"k", b"old");
        expiring.ttl = Some(Ttl { milliseconds: 10 });
        engine.put(expiring).expect("expiring value should fit");

        engine
            .put(put("default", b"k", b"new"))
            .expect("replacement should fit");

        clock.advance(11);
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Hit(b"new".to_vec())
        );
        assert_eq!(engine.stats().expirations, 0);
    }

    #[test]
    fn accounting_accessors_do_not_reclaim_expired_entries() {
        let (mut engine, clock) = engine();
        let mut expiring = put("default", b"k", b"value");
        expiring.ttl = Some(Ttl { milliseconds: 10 });
        expiring.cost = Some(99);
        engine.put(expiring).expect("expiring value should fit");

        let memory_before_expiry = engine.memory_used_bytes();
        clock.advance(11);

        assert_eq!(engine.len(), 1);
        assert_eq!(engine.memory_used_bytes(), memory_before_expiry);
        assert_eq!(engine.cost_score_total(), 99);
        assert_eq!(engine.stats().expirations, 0);

        assert_eq!(engine.reclaim_expired(), 1);
        assert_eq!(engine.len(), 0);
        assert_eq!(engine.memory_used_bytes(), 0);
        assert_eq!(engine.cost_score_total(), 0);
        assert_eq!(engine.stats().expirations, 1);
    }

    #[test]
    fn reclaim_expired_budget_limits_cleanup_work() {
        let (mut engine, clock) = engine();
        for key in [b"a", b"b", b"c"] {
            let mut command = put("default", key, b"value");
            command.ttl = Some(Ttl { milliseconds: 10 });
            engine.put(command).expect("expiring value should fit");
        }

        clock.advance(11);

        assert_eq!(engine.reclaim_expired_budget(1), 1);
        assert_eq!(engine.len(), 2);
        assert_eq!(engine.stats().expirations, 1);

        assert_eq!(engine.reclaim_expired_budget(1), 1);
        assert_eq!(engine.len(), 1);
        assert_eq!(engine.stats().expirations, 2);

        assert_eq!(engine.reclaim_expired_budget(16), 1);
        assert_eq!(engine.len(), 0);
        assert_eq!(engine.stats().expirations, 3);
    }

    #[test]
    fn cost_score_total_tracks_accounted_entries_until_reclaimed() {
        let (mut engine, clock) = engine();
        let mut first = put("default", b"a", b"value");
        first.cost = Some(10);
        engine.put(first).expect("put should fit");

        let mut second = put("default", b"b", b"value");
        second.cost = Some(7);
        second.ttl = Some(Ttl { milliseconds: 10 });
        engine.put(second).expect("put should fit");
        assert_eq!(engine.cost_score_total(), 17);

        let mut replacement = put("default", b"a", b"value");
        replacement.cost = Some(3);
        engine.put(replacement).expect("put should fit");
        assert_eq!(engine.cost_score_total(), 10);

        assert!(engine.delete("default", b"a"));
        assert_eq!(engine.cost_score_total(), 7);

        clock.advance(11);
        assert_eq!(engine.cost_score_total(), 7);
        assert_eq!(engine.reclaim_expired(), 1);
        assert_eq!(engine.cost_score_total(), 0);
    }

    #[test]
    fn reclaims_expired_entries_before_evicting_live_entries() {
        let (mut engine, clock) = tiny_engine(240, 1_000);
        let mut expiring = put("default", b"old", b"1111111111");
        expiring.ttl = Some(Ttl { milliseconds: 1 });
        engine.put(expiring).expect("old should fit");
        engine
            .put(put("default", b"live", b"2222222222"))
            .expect("live should fit");

        clock.advance(2);
        let outcome = engine
            .put(put("default", b"new", b"3333333333"))
            .expect("new should fit after expired cleanup");

        assert_eq!(outcome.evicted, 0);
        assert_eq!(engine.stats().expirations, 1);
        assert_eq!(engine.stats().evictions, 0);
        assert_eq!(engine.get("default", b"old"), GetOutcome::Miss);
        assert_eq!(
            engine.get("default", b"live"),
            GetOutcome::Hit(b"2222222222".to_vec())
        );
        assert_eq!(
            engine.get("default", b"new"),
            GetOutcome::Hit(b"3333333333".to_vec())
        );
    }

    #[test]
    fn lease_miss_grants_once_and_completion_writes_value() {
        let (mut engine, _) = engine();

        let first = engine.start_lease("default", b"k", 10_000);
        let token = match first {
            StartLeaseOutcome::LeaseGranted { token, stale_value } => {
                assert_eq!(stale_value, None);
                token
            }
            other => panic!("expected lease grant, got {other:?}"),
        };
        assert_eq!(
            engine.start_lease("default", b"k", 10_000),
            StartLeaseOutcome::LeaseDenied
        );

        engine
            .complete_lease(CompleteLeaseCommand {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_token: token,
                value: b"fresh".to_vec(),
                ttl: Some(Ttl { milliseconds: 100 }),
                stale_ttl: None,
                tags: Vec::new(),
                cost: None,
            })
            .expect("lease completion should put value");

        assert_eq!(
            engine.start_lease("default", b"k", 10_000),
            StartLeaseOutcome::Hit(b"fresh".to_vec())
        );
    }

    #[test]
    fn stale_lease_grant_includes_stale_value_and_active_lease_serves_stale() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"stale");
        command.ttl = Some(Ttl { milliseconds: 10 });
        command.stale_ttl = Some(Ttl { milliseconds: 100 });
        engine.put(command).expect("put should fit");

        clock.advance(11);
        let first = engine.start_lease("default", b"k", 10_000);
        let token = match first {
            StartLeaseOutcome::LeaseGranted { token, stale_value } => {
                assert_eq!(stale_value, Some(b"stale".to_vec()));
                token
            }
            other => panic!("expected stale lease grant, got {other:?}"),
        };
        assert_eq!(
            engine.start_lease("default", b"k", 10_000),
            StartLeaseOutcome::Stale {
                value: b"stale".to_vec()
            }
        );

        engine
            .complete_lease(CompleteLeaseCommand {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_token: token,
                value: b"fresh".to_vec(),
                ttl: Some(Ttl { milliseconds: 100 }),
                stale_ttl: None,
                tags: Vec::new(),
                cost: None,
            })
            .expect("lease completion should refresh value");
        assert_eq!(
            engine.get("default", b"k"),
            GetOutcome::Hit(b"fresh".to_vec())
        );
    }

    #[test]
    fn expired_lease_can_be_reacquired_and_old_token_is_rejected() {
        let (mut engine, clock) = engine();
        let first = engine.start_lease("default", b"k", 10);
        let old_token = match first {
            StartLeaseOutcome::LeaseGranted { token, .. } => token,
            other => panic!("expected lease grant, got {other:?}"),
        };

        clock.advance(11);
        let second = engine.start_lease("default", b"k", 10);
        let new_token = match second {
            StartLeaseOutcome::LeaseGranted { token, .. } => token,
            other => panic!("expected second lease grant, got {other:?}"),
        };
        assert_ne!(old_token, new_token);

        let error = engine
            .complete_lease(CompleteLeaseCommand {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_token: old_token,
                value: b"old".to_vec(),
                ttl: None,
                stale_ttl: None,
                tags: Vec::new(),
                cost: None,
            })
            .expect_err("old token should fail");
        assert_eq!(error, CompleteLeaseError::InvalidLeaseToken);
    }

    #[test]
    fn active_lease_denies_refresh_even_after_stale_value_expires() {
        let (mut engine, clock) = engine();
        let mut command = put("default", b"k", b"stale");
        command.ttl = Some(Ttl { milliseconds: 10 });
        command.stale_ttl = Some(Ttl { milliseconds: 10 });
        engine.put(command).expect("put should fit");

        clock.advance(11);
        assert!(matches!(
            engine.start_lease("default", b"k", 100),
            StartLeaseOutcome::LeaseGranted {
                stale_value: Some(_),
                ..
            }
        ));

        clock.advance(11);
        assert_eq!(
            engine.start_lease("default", b"k", 100),
            StartLeaseOutcome::LeaseDenied
        );

        clock.advance(100);
        assert!(matches!(
            engine.start_lease("default", b"k", 100),
            StartLeaseOutcome::LeaseGranted {
                stale_value: None,
                ..
            }
        ));
    }

    #[test]
    fn concurrent_miss_grants_exactly_one_lease() {
        const CLIENTS: usize = 32;

        let engine = Arc::new(Mutex::new(Engine::new()));
        let barrier = Arc::new(Barrier::new(CLIENTS));
        let mut handles = Vec::new();

        for _ in 0..CLIENTS {
            let engine = Arc::clone(&engine);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                engine
                    .lock()
                    .expect("engine lock")
                    .start_lease("default", b"shared-miss", 60_000)
            }));
        }

        let outcomes: Vec<StartLeaseOutcome> = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread should finish"))
            .collect();
        let grants = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, StartLeaseOutcome::LeaseGranted { .. }))
            .count();
        let denials = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, StartLeaseOutcome::LeaseDenied))
            .count();

        assert_eq!(grants, 1);
        assert_eq!(denials, CLIENTS - 1);
    }
}
