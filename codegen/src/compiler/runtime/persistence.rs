pub(crate) const PERSISTENCE_RUNTIME: &str = r#"const __LS_MAGIC: [u8; 8] = *b"LIGHTSS1";
const __LS_FILE_MODE: u32 = 0o600;
const __LS_VERSION: u32 = 8;
const __LS_PREVIOUS_VERSION: u32 = 7;
const __LS_OLD_VERSION: u32 = 6;
const __LS_WAL_VERSION: u32 = 3;
const __LS_LEGACY_WAL_VERSION: u32 = 5;
const __LS_WAL_HEADER: usize = 40;
const __LS_SNAPSHOT_CHUNK: usize = 1024 * 1024;
const __LS_MAX_CHUNK: usize = 512 * 1024 * 1024;
const __LS_MAX_WAL_RECORD: usize = 256 * 1024 * 1024;
const __LS_ARENA_CHUNK: usize = 256 * 1024;
const __LS_STAGING_MAX: usize = 256 * 1024;

struct __LsStaging {
    base: u64,
    buf: Vec<u8>,
}
const __LS_INLINE_TEXT: usize = 16;

#[derive(Clone)]
enum __LsText {
    Inline(u8, [u8; __LS_INLINE_TEXT]),
    Heap(Box<str>),
}

impl __LsText {
    fn as_str(&self) -> &str {
        match self {
            __LsText::Inline(length, bytes) => {
                unsafe { std::str::from_utf8_unchecked(&bytes[..*length as usize]) }
            }
            __LsText::Heap(value) => value,
        }
    }
}

impl Default for __LsText {
    fn default() -> Self {
        __LsText::Inline(0, [0; __LS_INLINE_TEXT])
    }
}

impl From<&str> for __LsText {
    fn from(value: &str) -> Self {
        if value.len() <= __LS_INLINE_TEXT {
            let mut bytes = [0; __LS_INLINE_TEXT];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            __LsText::Inline(value.len() as u8, bytes)
        } else {
            __LsText::Heap(value.into())
        }
    }
}

impl From<String> for __LsText {
    fn from(value: String) -> Self {
        __LsText::from(value.as_str())
    }
}

impl std::borrow::Borrow<str> for __LsText {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for __LsText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for __LsText {}

impl PartialOrd for __LsText {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for __LsText {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::hash::Hash for __LsText {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl std::fmt::Debug for __LsText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl std::fmt::Display for __LsText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug)]
struct __LsOrdF32(f32);

impl PartialEq for __LsOrdF32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}

impl Eq for __LsOrdF32 {}

impl std::hash::Hash for __LsOrdF32 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0.to_bits(), state);
    }
}

impl PartialOrd for __LsOrdF32 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for __LsOrdF32 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl From<f32> for __LsOrdF32 {
    fn from(value: f32) -> Self {
        __LsOrdF32(value)
    }
}

#[derive(Clone, Copy, Debug)]
struct __LsOrdF64(f64);

impl PartialEq for __LsOrdF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}

impl Eq for __LsOrdF64 {}

impl std::hash::Hash for __LsOrdF64 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0.to_bits(), state);
    }
}

impl PartialOrd for __LsOrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for __LsOrdF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl From<f64> for __LsOrdF64 {
    fn from(value: f64) -> Self {
        __LsOrdF64(value)
    }
}

const __LS_SLOT_LEN_BITS: u32 = 24;
const __LS_SLOT_LEN_MASK: u64 = (1 << __LS_SLOT_LEN_BITS) - 1;
const __LS_SLOT_OFFSET_LIMIT: u64 = 1 << (64 - __LS_SLOT_LEN_BITS);
const __LS_SLOT_LARGE: u64 = u64::MAX;
const __LS_SLOT_DELETED: u64 = u64::MAX - 1;

fn __ls_slot_pack(offset: u64, length: u64) -> u64 {
    debug_assert!(length <= __LS_SLOT_LEN_MASK && offset < __LS_SLOT_OFFSET_LIMIT);
    (offset << __LS_SLOT_LEN_BITS) | length
}

fn __ls_slot_unpack(packed: u64) -> (u64, u64) {
    (packed >> __LS_SLOT_LEN_BITS, packed & __LS_SLOT_LEN_MASK)
}

#[derive(Clone)]
struct __LsSlots<K: Ord> {
    entries: BTreeMap<K, u64>,
    large: BTreeMap<K, (u64, u64)>,
}

impl<K: Ord + Clone> __LsSlots<K> {
    fn new() -> Self {
        Self { entries: BTreeMap::new(), large: BTreeMap::new() }
    }

    fn insert(&mut self, key: K, offset: u64, length: u64) {
        let packed = if length <= __LS_SLOT_LEN_MASK && offset < __LS_SLOT_OFFSET_LIMIT {
            let packed = __ls_slot_pack(offset, length);
            if packed != __LS_SLOT_LARGE && packed != __LS_SLOT_DELETED {
                packed
            } else {
                self.large.insert(key.clone(), (offset, length));
                __LS_SLOT_LARGE
            }
        } else {
            self.large.insert(key.clone(), (offset, length));
            __LS_SLOT_LARGE
        };
        self.entries.insert(key, packed);
    }

    fn get(&self, key: &K) -> Option<(u64, u64)> {
        let packed = *self.entries.get(key)?;
        if packed == __LS_SLOT_LARGE {
            self.large.get(key).copied()
        } else {
            Some(__ls_slot_unpack(packed))
        }
    }

    fn remove(&mut self, key: &K) {
        if let Some(packed) = self.entries.remove(key) {
            if packed == __LS_SLOT_LARGE {
                self.large.remove(key);
            }
        }
    }

    fn large_get(&self, key: &K) -> Option<(u64, u64)> {
        self.large.get(key).copied()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(key, _)| key)
    }

    fn iter(&self) -> __LsSlotsIter<'_, K> {
        __LsSlotsIter { inner: self.entries.range(..) }
    }

    fn iter_from(&self, start: &K) -> __LsSlotsIter<'_, K> {
        __LsSlotsIter { inner: self.entries.range(start..) }
    }
}

impl<K: Ord + Clone + std::borrow::Borrow<str>> __LsSlots<K> {
    fn get_str(&self, key: &str) -> Option<(u64, u64)> {
        let packed = *self.entries.get(key)?;
        if packed == __LS_SLOT_LARGE {
            self.large.get(key).copied()
        } else {
            Some(__ls_slot_unpack(packed))
        }
    }

    fn remove_str(&mut self, key: &str) {
        if let Some(packed) = self.entries.remove(key) {
            if packed == __LS_SLOT_LARGE {
                self.large.remove(key);
            }
        }
    }

    fn contains_str(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
}

struct __LsSlotsIter<'a, K> {
    inner: std::collections::btree_map::Range<'a, K, u64>,
}

impl<'a, K: Clone> Iterator for __LsSlotsIter<'a, K> {
    type Item = (K, u64);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(key, packed)| (key.clone(), *packed))
    }
}

const __LS_DENSE_GAP: i64 = 262_144;

#[derive(Clone)]
enum __LsSlotsI64 {
    Dense { base: i64, entries: Vec<u64>, live: usize },
    Generic(__LsSlots<i64>),
}

impl __LsSlotsI64 {
    fn new() -> Self {
        Self::Dense { base: 0, entries: Vec::new(), live: 0 }
    }

    fn to_generic(&mut self) {
        if let Self::Dense { base, entries, .. } = self {
            let base = *base;
            let mut generic = __LsSlots::new();
            for (position, packed) in entries.drain(..).enumerate() {
                if packed != 0 {
                    let (offset, length) = __ls_slot_unpack(packed);
                    generic.insert(base + position as i64, offset, length);
                }
            }
            *self = Self::Generic(generic);
        }
    }

    fn insert(&mut self, key: i64, offset: u64, length: u64) {
        if length <= __LS_SLOT_LEN_MASK && offset < __LS_SLOT_OFFSET_LIMIT {
            if let Self::Dense { base, entries, live } = self {
                if entries.is_empty() {
                    *base = key;
                }
                if let Some(index) = key.checked_sub(*base) {
                    if (0..=__LS_DENSE_GAP).contains(&index) {
                        let index = index as usize;
                        if index >= entries.len() {
                            entries.resize(index + 1, 0);
                        }
                        let packed = __ls_slot_pack(offset, length);
                        if packed != __LS_SLOT_LARGE && packed != __LS_SLOT_DELETED {
                            if entries[index] == 0 {
                                *live += 1;
                            }
                            entries[index] = packed;
                            return;
                        }
                    }
                }
            }
        }
        self.to_generic();
        if let Self::Generic(generic) = self {
            generic.insert(key, offset, length);
        }
    }

    fn get(&self, key: &i64) -> Option<(u64, u64)> {
        match self {
            Self::Dense { base, entries, .. } => {
                let index = key.checked_sub(*base)?;
                if index < 0 {
                    return None;
                }
                let packed = *entries.get(index as usize)?;
                if packed == 0 {
                    None
                } else {
                    Some(__ls_slot_unpack(packed))
                }
            }
            Self::Generic(generic) => generic.get(key),
        }
    }

    fn remove(&mut self, key: &i64) {
        match self {
            Self::Dense { base, entries, live } => {
                if let Some(index) = key.checked_sub(*base) {
                    if index >= 0 {
                        if let Some(packed) = entries.get_mut(index as usize) {
                            if *packed != 0 {
                                *packed = 0;
                                *live = live.saturating_sub(1);
                            }
                        }
                    }
                }
            }
            Self::Generic(generic) => generic.remove(key),
        }
    }

    fn large_get(&self, key: &i64) -> Option<(u64, u64)> {
        match self {
            Self::Dense { .. } => None,
            Self::Generic(generic) => generic.large_get(key),
        }
    }

    fn contains_key(&self, key: &i64) -> bool {
        self.get(key).is_some()
    }

    fn len(&self) -> usize {
        match self {
            Self::Dense { live, .. } => *live,
            Self::Generic(generic) => generic.len(),
        }
    }

    fn keys(&self) -> impl Iterator<Item = i64> + '_ {
        self.iter().map(|(key, _)| key)
    }

    fn iter(&self) -> __LsSlotsI64Iter<'_> {
        __LsSlotsI64Iter {
            slots: self,
            position: 0,
            generic: match self {
                Self::Generic(generic) => Some(generic.iter()),
                Self::Dense { .. } => None,
            },
        }
    }

    fn iter_from(&self, start: &i64) -> __LsSlotsI64Iter<'_> {
        match self {
            Self::Dense { base, entries, .. } => __LsSlotsI64Iter {
                slots: self,
                position: ((*start).saturating_sub(*base).max(0) as usize).min(entries.len()),
                generic: None,
            },
            Self::Generic(generic) => __LsSlotsI64Iter {
                slots: self,
                position: 0,
                generic: Some(generic.iter_from(start)),
            },
        }
    }
}

struct __LsSlotsI64Iter<'a> {
    slots: &'a __LsSlotsI64,
    position: usize,
    generic: Option<__LsSlotsIter<'a, i64>>,
}

impl<'a> Iterator for __LsSlotsI64Iter<'a> {
    type Item = (i64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(generic) = &mut self.generic {
            return generic.next();
        }
        match self.slots {
            __LsSlotsI64::Dense { base, entries, .. } => loop {
                if self.position >= entries.len() {
                    return None;
                }
                let position = self.position;
                self.position += 1;
                let packed = entries[position];
                if packed != 0 {
                    return Some((*base + position as i64, packed));
                }
            },
            __LsSlotsI64::Generic(_) => None,
        }
    }
}

#[derive(Clone)]
struct __LsIndex<Eq: Ord, Key: Ord> {
    groups: BTreeMap<Eq, __LsGroup<Key>>,
}

#[derive(Clone)]
struct __LsGroup<Key: Ord> {
    entries: Vec<Key>,
    deleted: Vec<bool>,
    pending: Vec<Key>,
    dead: usize,
    shrink: bool,
}

impl<Eq: Ord, Key: Ord> __LsIndex<Eq, Key> {
    fn new() -> Self {
        Self { groups: BTreeMap::new() }
    }

    fn insert(&mut self, eq: Eq, key: Key) {
        let group = self.groups.entry(eq).or_insert_with(|| __LsGroup {
            entries: Vec::new(),
            deleted: Vec::new(),
            pending: Vec::new(),
            dead: 0,
            shrink: true,
        });
        if group.pending.len() == group.pending.capacity() && group.pending.len() >= 8192 {
            group.pending.reserve_exact(4096);
        }
        group.pending.push(key);
    }

    fn prepare(&mut self, eq: &Eq) {
        if let Some(group) = self.groups.get_mut(eq) {
            group.prepare();
        }
    }

    fn get(&self, eq: &Eq) -> Option<&__LsGroup<Key>> {
        self.groups.get(eq)
    }

    fn remove(&mut self, eq: &Eq, key: &Key) {
        let empty = match self.groups.get_mut(eq) {
            Some(group) => {
                if let Some(position) = group.pending.iter().position(|existing| existing == key) {
                    group.pending.swap_remove(position);
                } else if let Ok(position) = group.entries.binary_search(key) {
                    if !group.deleted[position] {
                        group.deleted[position] = true;
                        group.dead += 1;
                    }
                }
                group.entries.len() == group.dead && group.pending.is_empty()
            }
            None => false,
        };
        if empty {
            self.groups.remove(eq);
        }
    }
}

impl<Key: Ord> __LsGroup<Key> {
    fn prepare(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if self.entries.is_empty() {
            self.entries = std::mem::take(&mut self.pending);
            self.entries.sort_unstable();
            self.entries.dedup();
            self.deleted = vec![false; self.entries.len()];
            if self.shrink {
                self.entries.shrink_to_fit();
                self.shrink = false;
            }
            return;
        }
        if self.dead > 0 {
            let mut write = 0;
            for read in 0..self.entries.len() {
                if !self.deleted[read] {
                    if write != read {
                        self.entries.swap(write, read);
                    }
                    write += 1;
                }
            }
            self.entries.truncate(write);
            self.deleted.clear();
            self.deleted.resize(write, false);
            self.dead = 0;
        }
        if self.pending.len() <= 64 {
            for key in self.pending.drain(..) {
                match self.entries.binary_search(&key) {
                    Ok(position) => {
                        if self.deleted[position] {
                            self.deleted[position] = false;
                            self.dead -= 1;
                        }
                    }
                    Err(position) => {
                        self.entries.insert(position, key);
                        self.deleted.insert(position, false);
                    }
                }
            }
        } else {
            self.entries.extend(self.pending.drain(..));
            self.entries.sort_unstable();
            self.entries.dedup();
            self.deleted = vec![false; self.entries.len()];
            self.dead = 0;
        }
        self.pending = Vec::new();
    }

    fn values(&self) -> impl Iterator<Item = &Key> {
        self.entries
            .iter()
            .zip(self.deleted.iter())
            .filter_map(|(key, deleted)| (!deleted).then_some(key))
    }

    fn values_from<'a>(&'a self, start: &Key) -> impl Iterator<Item = &'a Key> {
        let position = self.entries.partition_point(|key| key < start);
        self.entries[position..]
            .iter()
            .zip(self.deleted[position..].iter())
            .filter_map(|(key, deleted)| (!deleted).then_some(key))
    }
}

fn __ls_hash_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn __ls_hash(bytes: &[u8]) -> u64 {
    __ls_hash_update(0xcbf29ce484222325u64, bytes)
}

fn __ls_hash_tagged(tag: u8, bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    hash ^= tag as u64;
    hash = hash.wrapping_mul(0x100000001b3);
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

struct __LsFx(u64);

impl __LsFx {
    fn new() -> Self {
        Self(0x9e3779b97f4a7c15)
    }
}

impl std::hash::Hasher for __LsFx {
    fn write(&mut self, bytes: &[u8]) -> () {
        let mut hash = self.0;
        for chunk in bytes.chunks(8) {
            let mut word = 0u64;
            for (position, byte) in chunk.iter().enumerate() {
                word |= (*byte as u64) << (position * 8);
            }
            word = word.wrapping_add(0x9e3779b97f4a7c15);
            word = word.wrapping_mul(0xbf58476d1ce4e5b9);
            hash ^= word;
            hash = hash.wrapping_mul(0xbf58476d1ce4e5b9);
        }
        self.0 = hash;
    }
    fn write_u8(&mut self, value: u8) -> () {
        self.0 ^= value as u64;
        self.0 = self.0.wrapping_mul(0xbf58476d1ce4e5b9);
    }
    fn write_u64(&mut self, value: u64) -> () {
        self.0 ^= value.wrapping_add(0x9e3779b97f4a7c15);
        self.0 = self.0.wrapping_mul(0xbf58476d1ce4e5b9);
    }
    fn write_i64(&mut self, value: i64) -> () {
        self.write_u64(value as u64);
    }
    fn finish(&self) -> u64 {
        let mut hash = self.0;
        hash ^= hash >> 29;
        hash = hash.wrapping_mul(0x165667b19e3779f9);
        hash ^= hash >> 32;
        hash
    }
}

fn __ls_shard16<T: std::hash::Hash>(key: &T) -> usize {
    let mut hasher = __LsFx::new();
    std::hash::Hash::hash(key, &mut hasher);
    (std::hash::Hasher::finish(&hasher) as usize) & 15
}

fn __ls_spill_read(file: &File, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(bytes, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut position = 0;
        while position < bytes.len() {
            let read = file.seek_read(&mut bytes[position..], offset + position as u64)?;
            if read == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "spill read failed"));
            }
            position += read;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, offset, bytes);
        Err(io::Error::other("spill reads are not supported on this platform"))
    }
}

fn __ls_spill_write(file: &File, offset: u64, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(bytes, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut position = 0;
        while position < bytes.len() {
            let wrote = file.seek_write(&bytes[position..], offset + position as u64)?;
            if wrote == 0 {
                return Err(io::Error::other("spill write failed"));
            }
            position += wrote;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, offset, bytes);
        Err(io::Error::other("spill writes are not supported on this platform"))
    }
}

fn __ls_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn __ls_private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(__LS_FILE_MODE);
    }
    options
}

fn __ls_open_private_file(path: &Path) -> io::Result<File> {
    for _ in 0..4 {
        match fs::symlink_metadata(path) {
            Ok(before) => {
                if before.file_type().is_symlink() || !before.file_type().is_file() {
                    return Err(__ls_invalid(&format!(
                        "refusing non-regular file `{}`",
                        path.display()
                    )));
                }
                let file = __ls_private_options()
                    .read(true)
                    .write(true)
                    .open(path)?;
                let after = file.metadata()?;
                if __ls_metadata_identity(&before) != __ls_metadata_identity(&after) {
                    return Err(__ls_invalid(&format!(
                        "file `{}` changed while opening",
                        path.display()
                    )));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(__LS_FILE_MODE))?;
                }
                return Ok(file);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match __ls_private_options()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
                {
                    Ok(file) => {
                        __ls_sync_parent(path)?;
                        return Ok(file);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("could not safely open sidecar file"))
}

fn __ls_full_sync_requested() -> bool {
    matches!(
        std::env::var("LIGHTSHADE_FULL_SYNC").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[cfg(target_os = "macos")]
fn __ls_barrier_sync(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const F_BARRIERFSYNC: i32 = 86;
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn fsync(fd: i32) -> i32;
    }
    let fd = file.as_raw_fd();
    if unsafe { fcntl(fd, F_BARRIERFSYNC) } != -1 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(25) | Some(22) => {}
        _ => return Err(error),
    }
    if unsafe { fsync(fd) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn __ls_sync_file(file: &File, data_only: bool) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if !__ls_full_sync_requested() {
            return __ls_barrier_sync(file);
        }
    }
    if data_only {
        file.sync_data()
    } else {
        file.sync_all()
    }
}

fn __ls_sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn __ls_wal_record_length(operations: usize) -> io::Result<u32> {
    let length = operations.checked_add(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "WAL record is too large")
    })?;
    if length > __LS_MAX_WAL_RECORD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "WAL record is too large: {length} bytes exceeds the {} byte limit; split the write into smaller transactions",
                __LS_MAX_WAL_RECORD
            ),
        ));
    }
    u32::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WAL record is too large"))
}

fn __ls_invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(unix)]
fn __ls_metadata_identity(metadata: &fs::Metadata) -> (u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino(), metadata.len())
}

#[cfg(windows)]
fn __ls_metadata_identity(metadata: &fs::Metadata) -> (u64, u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (
        metadata.len(),
        metadata.creation_time(),
        metadata.last_write_time(),
    )
}

#[cfg(not(any(unix, windows)))]
fn __ls_metadata_identity(metadata: &fs::Metadata) -> (u64, u64, u64) {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos() as u64)
        .unwrap_or(0);
    (metadata.len(), modified, 0)
}

fn __ls_file_identity(path: &Path) -> io::Result<Option<(u64, u64, u64)>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(__ls_metadata_identity(&metadata))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn __ls_put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_i8(out: &mut Vec<u8>, value: i8) {
    out.push(value as u8);
}

fn __ls_put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_var_u64(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn __ls_put_var_i64(out: &mut Vec<u8>, value: i64) {
    __ls_put_var_u64(out, ((value << 1) ^ (value >> 63)) as u64);
}

fn __ls_put_var_text(out: &mut Vec<u8>, value: &str) {
    __ls_put_var_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn __ls_put_var_bytes(out: &mut Vec<u8>, value: &[u8]) {
    __ls_put_var_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn __ls_put_bool(out: &mut Vec<u8>, value: bool) {
    out.push(if value { 1 } else { 0 });
}

fn __ls_put_text(out: &mut Vec<u8>, value: &str) {
    __ls_put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

fn __ls_put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    __ls_put_u32(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn __ls_put_uuid(out: &mut Vec<u8>, value: &[u8; 16]) {
    out.extend_from_slice(value);
}

fn __ls_put_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn __ls_put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

#[derive(Clone, Copy)]
enum __LsValue<'a> {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Text(&'a str),
    Bytes(&'a [u8]),
    Uuid(&'a [u8; 16]),
    Null(u8),
}

fn __ls_put_value(out: &mut Vec<u8>, value: __LsValue<'_>) {
    match value {
        __LsValue::I8(value) => {
            out.push(3);
            __ls_put_i8(out, value);
        }
        __LsValue::I16(value) => {
            out.push(4);
            __ls_put_i16(out, value);
        }
        __LsValue::I32(value) => {
            out.push(5);
            __ls_put_i32(out, value);
        }
        __LsValue::I64(value) => {
            out.push(0);
            __ls_put_i64(out, value);
        }
        __LsValue::U8(value) => {
            out.push(6);
            __ls_put_u8(out, value);
        }
        __LsValue::U16(value) => {
            out.push(7);
            __ls_put_u16(out, value);
        }
        __LsValue::U32(value) => {
            out.push(8);
            __ls_put_u32(out, value);
        }
        __LsValue::U64(value) => {
            out.push(9);
            __ls_put_u64(out, value);
        }
        __LsValue::F32(value) => {
            out.push(12);
            __ls_put_f32(out, value);
        }
        __LsValue::F64(value) => {
            out.push(13);
            __ls_put_f64(out, value);
        }
        __LsValue::Bool(value) => {
            out.push(1);
            __ls_put_bool(out, value);
        }
        __LsValue::Text(value) => {
            out.push(2);
            __ls_put_text(out, value);
        }
        __LsValue::Bytes(value) => {
            out.push(10);
            __ls_put_bytes(out, value);
        }
        __LsValue::Uuid(value) => {
            out.push(11);
            __ls_put_uuid(out, value);
        }
        __LsValue::Null(tag) => {
            out.push(14);
            out.push(tag);
        }
    }
}

struct __LsReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> __LsReader<'a> {
    fn new(bytes: &'a [u8]) -> __LsReader<'a> {
        __LsReader { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| __ls_invalid("length overflow"))?;
        if end > self.bytes.len() {
            return Err(__ls_invalid("unexpected end of data"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn i8(&mut self) -> io::Result<i8> {
        Ok(self.u8()? as i8)
    }

    fn i16(&mut self) -> io::Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn var_u64(&mut self) -> io::Result<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(__ls_invalid("invalid varint"))
    }

    fn var_i64(&mut self) -> io::Result<i64> {
        let value = self.var_u64()?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }

    fn var_text(&mut self) -> io::Result<String> {
        let length = self.var_u64()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| __ls_invalid("invalid UTF-8"))
    }

    fn var_bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = self.var_u64()? as usize;
        Ok(self.take(length)?.to_vec())
    }

    fn bool(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(__ls_invalid("invalid boolean")),
        }
    }

    fn text(&mut self) -> io::Result<String> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| __ls_invalid("invalid UTF-8"))
    }

    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = self.u32()? as usize;
        Ok(self.take(length)?.to_vec())
    }

    fn uuid(&mut self) -> io::Result<[u8; 16]> {
        Ok(self.take(16)?.try_into().unwrap())
    }

    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn owned_value(&mut self) -> io::Result<__LsOwnedValue> {
        match self.u8()? {
            0 => Ok(__LsOwnedValue::I64(self.i64()?)),
            1 => Ok(__LsOwnedValue::Bool(self.bool()?)),
            2 => Ok(__LsOwnedValue::Text(self.text()?)),
            3 => Ok(__LsOwnedValue::I8(self.i8()?)),
            4 => Ok(__LsOwnedValue::I16(self.i16()?)),
            5 => Ok(__LsOwnedValue::I32(self.i32()?)),
            6 => Ok(__LsOwnedValue::U8(self.u8()?)),
            7 => Ok(__LsOwnedValue::U16(self.u16()?)),
            8 => Ok(__LsOwnedValue::U32(self.u32()?)),
            9 => Ok(__LsOwnedValue::U64(self.u64()?)),
            10 => Ok(__LsOwnedValue::Bytes(self.bytes()?)),
            11 => Ok(__LsOwnedValue::Uuid(self.uuid()?)),
            12 => Ok(__LsOwnedValue::F32(self.f32()?)),
            13 => Ok(__LsOwnedValue::F64(self.f64()?)),
            14 => {
                let tag = self.u8()?;
                if tag > 13 {
                    return Err(__ls_invalid("unknown null type tag"));
                }
                Ok(__LsOwnedValue::Null(tag))
            }
            _ => Err(__ls_invalid("unknown WAL value tag")),
        }
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }
}

struct __LsInput<R> {
    inner: R,
    hash: u64,
}

impl<R: Read> __LsInput<R> {
    fn new(inner: R) -> __LsInput<R> {
        __LsInput {
            inner,
            hash: 0xcbf29ce484222325u64,
        }
    }

    fn take(&mut self, count: usize) -> io::Result<Vec<u8>> {
        if count > __LS_MAX_CHUNK {
            return Err(__ls_invalid("snapshot value is too large"));
        }
        let mut bytes = vec![0u8; count];
        self.inner.read_exact(&mut bytes).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                __ls_invalid("snapshot is truncated")
            } else {
                error
            }
        })?;
        self.hash = __ls_hash_update(self.hash, &bytes);
        Ok(bytes)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn i8(&mut self) -> io::Result<i8> {
        Ok(self.u8()? as i8)
    }

    fn i16(&mut self) -> io::Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn bool(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(__ls_invalid("invalid boolean")),
        }
    }

    fn text(&mut self) -> io::Result<String> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes).map_err(|_| __ls_invalid("invalid UTF-8"))
    }

    fn finish(&mut self) -> io::Result<()> {
        let mut expected = [0u8; 8];
        self.inner.read_exact(&mut expected).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                __ls_invalid("snapshot is truncated")
            } else {
                error
            }
        })?;
        let mut trailing = [0u8; 1];
        if self.inner.read(&mut trailing)? != 0 {
            return Err(__ls_invalid("snapshot has trailing bytes"));
        }
        if u64::from_le_bytes(expected) != self.hash {
            return Err(__ls_invalid("snapshot checksum mismatch"));
        }
        Ok(())
    }
}

fn __ls_emit<W: Write>(writer: &mut W, hash: &mut u64, bytes: &[u8]) -> io::Result<()> {
    *hash = __ls_hash_update(*hash, bytes);
    writer.write_all(bytes)
}

fn __ls_emit_chunk<W: Write>(
    writer: &mut W,
    hash: &mut u64,
    rows: u32,
    chunk: &[u8],
) -> io::Result<()> {
    let mut header = Vec::new();
    __ls_put_u32(&mut header, chunk.len() as u32);
    __ls_put_u32(&mut header, rows);
    __ls_put_u64(&mut header, __ls_hash(chunk));
    __ls_emit(writer, hash, &header)?;
    __ls_emit(writer, hash, chunk)
}

fn __ls_read_chunk<R: Read>(
    input: &mut __LsInput<R>,
    rows: u64,
    parsed: &mut u64,
) -> io::Result<(Vec<u8>, u32)> {
    let length = input.u32()? as usize;
    let chunk_rows = input.u32()?;
    if chunk_rows == 0 || chunk_rows as u64 > rows - *parsed {
        return Err(__ls_invalid("snapshot chunk row count mismatch"));
    }
    if length > __LS_MAX_CHUNK {
        return Err(__ls_invalid("snapshot chunk is too large"));
    }
    let expected = input.u64()?;
    let payload = input.take(length)?;
    if __ls_hash(&payload) != expected {
        return Err(__ls_invalid("snapshot chunk checksum mismatch"));
    }
    *parsed += chunk_rows as u64;
    Ok((payload, chunk_rows))
}

enum __LsOwnedValue {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Uuid([u8; 16]),
    Null(u8),
}

struct __LsRawTable {
    columns: Vec<(String, u8, bool, bool)>,
    rows: Vec<Vec<__LsOwnedValue>>,
}

struct __LsWalState {
    wal: io::BufWriter<File>,
    bytes: u64,
    lsn: u64,
    durable: u64,
    deferred: bool,
    failed: Option<String>,
    alive: bool,
    sealed: bool,
}

pub struct __LsWalSync {
    mutex: std::sync::Mutex<__LsWalState>,
    cond: std::sync::Condvar,
}

pub fn __ls_wait_on_sync(sync: &std::sync::Arc<__LsWalSync>, lsn: u64) -> io::Result<()> {
    let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
    loop {
        if guard.durable >= lsn {
            return Ok(());
        }
        if let Some(error) = guard.failed.clone() {
            return Err(io::Error::other(error));
        }
        if !guard.alive {
            if guard.sealed {
                return Err(io::Error::other("database is closed"));
            }
            match __ls_sync_flush(&mut guard, lsn) {
                Ok(()) => {
                    sync.cond.notify_all();
                    return Ok(());
                }
                Err(error) => {
                    guard.failed = Some(error.to_string());
                    sync.cond.notify_all();
                    return Err(error);
                }
            }
        }
        guard = sync
            .cond
            .wait_timeout(guard, std::time::Duration::from_millis(100))
            .map_err(|_| __ls_wal_poisoned())?
            .0;
    }
}

struct __LsStore {
    path: PathBuf,
    sync: std::sync::Arc<__LsWalSync>,
    sync_each: bool,
    snapshot_lsn: u64,
    checkpoint_at: u64,
    _lock: File,
}

fn __ls_wal_poisoned() -> io::Error {
    io::Error::other("wal lock poisoned")
}

fn __ls_group_interval() -> std::time::Duration {
    match std::env::var("LIGHTSHADE_GROUP_COMMIT_US")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(value) => std::time::Duration::from_micros(value.max(1).min(1_000_000)),
        None => std::time::Duration::from_micros(1_000),
    }
}

fn __ls_sync_flush(guard: &mut __LsWalState, target: u64) -> io::Result<()> {
    guard.wal.flush()?;
    __ls_sync_file(guard.wal.get_ref(), true)?;
    guard.durable = guard.durable.max(target);
    Ok(())
}

fn __ls_sync_take_file(guard: &mut __LsWalState) -> io::Result<File> {
    guard.wal.flush()?;
    guard.wal.get_ref().try_clone()
}

fn __ls_sync_loop(sync: std::sync::Arc<__LsWalSync>, shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let interval = __ls_group_interval();
    loop {
        let (file, target, draining) = {
            let mut guard = match sync.mutex.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    let mut guard = poisoned.into_inner();
                    guard.failed = Some("wal lock poisoned".to_string());
                    guard.alive = false;
                    sync.cond.notify_all();
                    return;
                }
            };
            while !guard.sealed
                && !shutdown.load(std::sync::atomic::Ordering::SeqCst)
                && (guard.bytes == 0 || guard.durable >= guard.lsn)
            {
                guard = match sync.cond.wait_timeout(guard, interval) {
                    Ok((guard, _)) => guard,
                    Err(poisoned) => {
                        let mut guard = poisoned.into_inner().0;
                        guard.failed = Some("wal lock poisoned".to_string());
                        guard.alive = false;
                        sync.cond.notify_all();
                        return;
                    }
                };
            }
            let draining = guard.sealed || shutdown.load(std::sync::atomic::Ordering::SeqCst);
            let target = guard.lsn;
            match __ls_sync_take_file(&mut guard) {
                Ok(file) => (file, target, draining),
                Err(error) => {
                    guard.failed = Some(error.to_string());
                    guard.alive = false;
                    drop(guard);
                    sync.cond.notify_all();
                    return;
                }
            }
        };
        let result = __ls_sync_file(&file, true);
        let mut guard = match sync.mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.failed = Some("wal lock poisoned".to_string());
                guard.alive = false;
                sync.cond.notify_all();
                return;
            }
        };
        match result {
            Ok(()) => {
                guard.durable = guard.durable.max(target);
            }
            Err(error) => {
                guard.failed = Some(error.to_string());
                guard.alive = false;
            }
        }
        let finished = guard.failed.is_some() || draining;
        if finished {
            guard.alive = false;
        }
        drop(guard);
        sync.cond.notify_all();
        if finished {
            return;
        }
    }
}

fn __ls_checkpoint_threshold() -> u64 {
    match std::env::var("LIGHTSHADE_CHECKPOINT_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(0) => u64::MAX,
        Some(value) => value.max(1),
        None => 8 * 1024 * 1024,
    }
}

fn __ls_row_quota() -> u64 {
    std::env::var("LIGHTSHADE_MAX_ROWS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn __ls_wal_quota() -> u64 {
    std::env::var("LIGHTSHADE_MAX_WAL_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn __ls_row_cache_budget() -> u64 {
    match std::env::var("LIGHTSHADE_ROW_CACHE_BYTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(value) => value,
        None => 64 * 1024 * 1024,
    }
}

fn __ls_spill_dir() -> io::Result<PathBuf> {
    let configured = std::env::var("LIGHTSHADE_SPILL_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match configured {
        None => Ok(std::env::temp_dir()),
        Some(dir) => {
            let path = PathBuf::from(&dir);
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => Ok(path),
                _ => Err(io::Error::other(format!(
                    "LIGHTSHADE_SPILL_DIR `{dir}` is not an existing directory"
                ))),
            }
        }
    }
}

fn __ls_encode_upsert(payload: &mut Vec<u8>, table: &str, columns: &[(&str, __LsValue<'_>)]) {
    payload.push(0);
    __ls_put_text(payload, table);
    __ls_put_u32(payload, columns.len() as u32);
    for (name, value) in columns {
        __ls_put_text(payload, name);
        __ls_put_value(payload, *value);
    }
}

fn __ls_encode_delete(payload: &mut Vec<u8>, table: &str, key: __LsValue<'_>) {
    payload.push(1);
    __ls_put_text(payload, table);
    __ls_put_value(payload, key);
}

fn __ls_replay_wal<R: io::Read + io::Seek>(
    db: &mut Db,
    wal: &mut R,
    snapshot_lsn: u64,
    limit: Option<u64>,
    migrate: bool,
) -> io::Result<(u64, u64)> {
    wal.seek(SeekFrom::Start(0))?;
    let mut position = 0u64;
    let mut valid = 0u64;
    let mut max_lsn = snapshot_lsn;
    loop {
        if let Some(limit) = limit {
            if position + __LS_WAL_HEADER as u64 > limit {
                break;
            }
        }
        let mut header = [0u8; __LS_WAL_HEADER];
        match wal.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                if let Some(limit) = limit {
                    if position + __LS_WAL_HEADER as u64 <= limit {
                        return Err(__ls_invalid("WAL was truncated during read"));
                    }
                }
                break;
            }
            Err(error) => return Err(error),
        }
        if header[..8] != __LS_MAGIC {
            return Err(__ls_invalid("bad WAL magic"));
        }
        let version = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let old_operations = version == __LS_LEGACY_WAL_VERSION;
        let old_effects = version == 2;
        if version != __LS_WAL_VERSION && !old_operations && !old_effects {
            return Err(__ls_invalid("unsupported WAL version"));
        }
        let fingerprint = u64::from_le_bytes(header[12..20].try_into().unwrap());
        if version == __LS_WAL_VERSION && !migrate && fingerprint != __LS_SCHEMA {
            return Err(__ls_invalid("WAL schema mismatch"));
        }
        if old_operations && fingerprint != __LS_WORKLOAD {
            return Err(__ls_invalid("WAL workload mismatch"));
        }
        let length = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        if length > __LS_MAX_WAL_RECORD {
            return Err(__ls_invalid("WAL record is too large"));
        }
        let record_end = position + __LS_WAL_HEADER as u64 + length as u64;
        if let Some(limit) = limit {
            if record_end > limit {
                break;
            }
        }
        let lsn = u64::from_le_bytes(header[24..32].try_into().unwrap());
        let checksum = u64::from_le_bytes(header[32..40].try_into().unwrap());
        let mut payload = vec![0u8; length];
        match wal.read_exact(&mut payload) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                if let Some(limit) = limit {
                    if record_end <= limit {
                        return Err(__ls_invalid("WAL was truncated during read"));
                    }
                }
                break;
            }
            Err(error) => return Err(error),
        }
        if __ls_hash(&payload) != checksum {
            return Err(__ls_invalid("WAL checksum mismatch"));
        }
        if lsn > snapshot_lsn {
            if lsn != max_lsn + 1 {
                return Err(__ls_invalid("WAL LSN gap"));
            }
            let mut reader = __LsReader::new(&payload);
            if old_operations {
                match reader.u8()? {
                    0 => db.__ls_replay_op(&mut reader)?,
                    1 => {
                        while !reader.is_empty() {
                            db.__ls_replay_op(&mut reader)?;
                        }
                    }
                    _ => return Err(__ls_invalid("unknown WAL record kind")),
                }
            } else {
                match reader.u8()? {
                    0 => db.__ls_replay_effect(&mut reader)?,
                    1 => {
                        while !reader.is_empty() {
                            db.__ls_replay_effect(&mut reader)?;
                        }
                    }
                    _ => return Err(__ls_invalid("unknown WAL record kind")),
                }
            }
            if !reader.is_empty() {
                return Err(__ls_invalid("WAL record has trailing bytes"));
            }
            max_lsn = lsn;
        }
        position += __LS_WAL_HEADER as u64 + length as u64;
        valid = position;
    }
    Ok((max_lsn, valid))
}

impl Db {
    fn __ls_acquire_lock(path: &Path) -> io::Result<File> {
        let lock_path = __ls_sidecar(path, ".lock");
        let lock = __ls_open_private_file(&lock_path)?;
        if let Err(error) = lock.try_lock() {
            return match error {
                fs::TryLockError::WouldBlock => Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "database is already open by another process",
                )),
                fs::TryLockError::Error(error) => Err(error),
            };
        }
        Ok(lock)
    }

    fn __ls_open(path: &Path, sync_each: bool, migrate: bool) -> io::Result<Db> {
        let lock = Db::__ls_acquire_lock(path)?;
        Db::__ls_open_locked(path, sync_each, migrate, lock)
    }

    fn __ls_open_locked(
        path: &Path,
        sync_each: bool,
        migrate: bool,
        lock: File,
    ) -> io::Result<Db> {
        let (mut db, snapshot_lsn) = if path.exists() {
            Db::__ls_read_snapshot(io::BufReader::new(File::open(path)?), migrate)?
        } else {
            (Db::__ls_new_spilled()?, 0)
        };
        let wal_path = __ls_sidecar(path, ".wal");
        let mut wal = __ls_open_private_file(&wal_path)?;
        let (max_lsn, valid_length) = __ls_replay_wal(&mut db, &mut wal, snapshot_lsn, None, migrate)?;
        let replayed = max_lsn > snapshot_lsn;
        db.__ls_lsn = max_lsn;
        wal.set_len(valid_length)?;
        wal.seek(SeekFrom::End(0))?;
        db.__ls_store = Some(__LsStore {
            path: path.to_path_buf(),
            sync: std::sync::Arc::new(__LsWalSync {
                mutex: std::sync::Mutex::new(__LsWalState {
                    wal: io::BufWriter::with_capacity(64 * 1024, wal),
                    bytes: valid_length,
                    lsn: max_lsn,
                    durable: max_lsn,
                    deferred: false,
                    failed: None,
                    alive: false,
                    sealed: false,
                }),
                cond: std::sync::Condvar::new(),
            }),
            sync_each,
            snapshot_lsn,
            checkpoint_at: __ls_checkpoint_threshold(),
            _lock: lock,
        });
        if replayed {
            db.checkpoint()?;
        }
        Ok(db)
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Db> {
        Db::__ls_open(path.as_ref(), true, false)
    }

    pub fn open_relaxed(path: impl AsRef<Path>) -> io::Result<Db> {
        Db::__ls_open(path.as_ref(), false, false)
    }

    pub fn recover(path: impl AsRef<Path>, discard_wal: bool) -> io::Result<Db> {
        let path = path.as_ref();
        let lock = Db::__ls_acquire_lock(path)?;
        let quarantine = if discard_wal {
            if !path.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot discard a WAL without a snapshot to validate",
                ));
            }
            Db::__ls_read_snapshot(io::BufReader::new(File::open(path)?), false)?;
            let wal_path = __ls_sidecar(path, ".wal");
            if wal_path.exists() {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0);
                let quarantine = PathBuf::from(format!(
                    "{}.discarded-{}-{nonce}",
                    wal_path.display(),
                    std::process::id()
                ));
                fs::rename(&wal_path, &quarantine)?;
                Some(quarantine)
            } else {
                None
            }
        } else {
            None
        };
        match Db::__ls_open_locked(path, true, false, lock) {
            Ok(db) => {
                if let Some(quarantine) = quarantine {
                    let _ = fs::remove_file(quarantine);
                }
                Ok(db)
            }
            Err(error) => {
                if let Some(quarantine) = quarantine {
                    let wal_path = __ls_sidecar(path, ".wal");
                    let _ = fs::rename(quarantine, wal_path);
                }
                Err(error)
            }
        }
    }

    pub fn open_migrated(path: impl AsRef<Path>) -> io::Result<Db> {
        let path = path.as_ref();
        let wal_path = __ls_sidecar(path, ".wal");
        if let Ok(metadata) = fs::metadata(&wal_path) {
            if metadata.len() >= 12 {
                let mut header = [0u8; 12];
                File::open(&wal_path)?.read_exact(&mut header)?;
                let version = u32::from_le_bytes(header[8..12].try_into().unwrap());
                if version == __LS_LEGACY_WAL_VERSION {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "WAL contains records from the previous build; recover with the previous build before migrating",
                    ));
                }
            }
        }
        let mut db = Db::__ls_open(path, true, true)?;
        db.checkpoint()?;
        Ok(db)
    }

    fn __ls_open_reader(path: &Path) -> io::Result<Db> {
        let wal_path = __ls_sidecar(path, ".wal");
        let identity_before = __ls_file_identity(path)?;
        let (mut wal, wal_length) = match File::open(&wal_path) {
            Ok(file) => {
                let length = file.metadata()?.len();
                (Some(file), length)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (None, 0),
            Err(error) => return Err(error),
        };
        let (mut db, snapshot_lsn) = match File::open(path) {
            Ok(file) => Db::__ls_read_snapshot(io::BufReader::new(file), true)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Db::__ls_new_spilled()?, 0),
            Err(error) => return Err(error),
        };
        let identity_after = __ls_file_identity(path)?;
        if identity_before != identity_after {
            return Err(__ls_invalid("snapshot moved during read"));
        }
        db.__ls_lsn = snapshot_lsn;
        if let Some(wal) = wal.as_mut() {
            let (max_lsn, _) = __ls_replay_wal(&mut db, wal, snapshot_lsn, Some(wal_length), false)?;
            db.__ls_lsn = max_lsn;
        }
        db.__ls_read_only = true;
        Ok(db)
    }

    pub fn open_read_only(path: impl AsRef<Path>) -> io::Result<Db> {
        let path = path.as_ref();
        let mut last = io::Error::other("reader could not obtain a consistent state");
        for attempt in 0..16 {
            match Db::__ls_open_reader(path) {
                Ok(db) => return Ok(db),
                Err(error) => {
                    let retry = matches!(
                        error.to_string().as_str(),
                        "WAL LSN gap"
                            | "WAL checksum mismatch"
                            | "WAL was truncated during read"
                            | "snapshot moved during read"
                    );
                    last = error;
                    if !retry {
                        break;
                    }
                    if attempt < 15 {
                        std::thread::yield_now();
                    }
                }
            }
        }
        Err(last)
    }

    fn __ls_ready(&self) -> io::Result<()> {
        match &self.__ls_broken {
            Some(message) => Err(io::Error::other(format!(
                "database is in a failed state: {message}; reopen the path"
            ))),
            None => Ok(()),
        }
    }

    fn __ls_writable(&self) -> io::Result<()> {
        if self.__ls_read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "database handle is read-only",
            ));
        }
        self.__ls_ready()
    }

    fn __ls_broken_message(&self) -> Option<&str> {
        self.__ls_broken.as_deref()
    }

    fn __ls_check_row_limit(&self, rows: u64) -> io::Result<()> {
        if self.__ls_max_rows != 0 && rows > self.__ls_max_rows {
            return Err(io::Error::other(format!(
                "row quota exceeded: {rows} rows would exceed LIGHTSHADE_MAX_ROWS={}",
                self.__ls_max_rows
            )));
        }
        Ok(())
    }

    fn __ls_check_row_quota(&self) -> io::Result<()> {
        if self.__ls_max_rows == 0 {
            return Ok(());
        }
        self.__ls_check_row_limit(self.__ls_row_count().saturating_add(1))
    }

    fn __ls_wal_quota_error(&self, operations: usize) -> Option<String> {
        if self.__ls_max_wal_bytes == 0 {
            return None;
        }
        let store = self.__ls_store.as_ref()?;
        let sync = std::sync::Arc::clone(&store.sync);
        let bytes = sync.mutex.lock().map(|guard| guard.bytes).unwrap_or(0);
        let incoming = (__LS_WAL_HEADER as u64)
            .saturating_add(1)
            .saturating_add(operations as u64);
        let total = bytes.saturating_add(incoming);
        if total > self.__ls_max_wal_bytes {
            Some(format!(
                "WAL quota exceeded: {total} bytes would exceed LIGHTSHADE_MAX_WAL_BYTES={}; checkpoint or raise the limit",
                self.__ls_max_wal_bytes
            ))
        } else {
            None
        }
    }

    fn __ls_effect_upsert(&mut self, table: &str, columns: &[(&str, __LsValue<'_>)]) {
        if self.__ls_broken.is_some() {
            return;
        }
        if self.__ls_store.is_none() && !self.__ls_in_transaction {
            return;
        }
        __ls_encode_upsert(&mut self.__ls_effects, table, columns);
        self.__ls_limit_effects();
    }

    fn __ls_effect_delete(&mut self, table: &str, key: __LsValue<'_>) {
        if self.__ls_broken.is_some() {
            return;
        }
        if self.__ls_store.is_none() && !self.__ls_in_transaction {
            return;
        }
        __ls_encode_delete(&mut self.__ls_effects, table, key);
        self.__ls_limit_effects();
    }

    fn __ls_limit_effects(&mut self) {
        if self.__ls_effects.len() <= __LS_MAX_WAL_RECORD {
            return;
        }
        self.__ls_effects.clear();
        self.__ls_broken = Some(format!(
            "transaction exceeds the {} byte WAL record limit; split it into smaller transactions",
            __LS_MAX_WAL_RECORD
        ));
    }

    fn __ls_finish_effects(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if self.__ls_in_transaction || self.__ls_effects.is_empty() {
            return Ok(());
        }
        let effects = std::mem::take(&mut self.__ls_effects);
        if self.__ls_store.is_none() {
            return Ok(());
        }
        if let Some(message) = self.__ls_wal_quota_error(effects.len()) {
            if let Err(error) = self.__ls_reload() {
                self.__ls_broken = Some(error.to_string());
                return Err(error);
            }
            return Err(io::Error::other(message));
        }
        match self.__ls_write_batch(&effects) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.__ls_broken = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn begin(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if self.__ls_read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "database handle is read-only",
            ));
        }
        if self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "transaction already active",
            ));
        }
        if self.__ls_store.is_none() {
            let mut backup = Vec::new();
            self.__ls_write_snapshot(&mut backup)?;
            self.__ls_memory_backup = Some(backup);
            self.__ls_in_transaction = true;
            self.__ls_effects.clear();
            return Ok(());
        }
        self.__ls_in_transaction = true;
        self.__ls_effects.clear();
        Ok(())
    }

    pub fn __ls_current_lsn(&self) -> u64 {
        self.__ls_lsn
    }

    fn __ls_wal_sync(&self) -> Option<std::sync::Arc<__LsWalSync>> {
        self.__ls_store
            .as_ref()
            .map(|store| std::sync::Arc::clone(&store.sync))
    }

    pub fn __ls_wait_durable(&self, lsn: u64) -> io::Result<()> {
        let Some(sync) = self.__ls_wal_sync() else {
            return Ok(());
        };
        __ls_wait_on_sync(&sync, lsn)
    }

    pub fn __ls_wal_sync_handle(&self) -> Option<std::sync::Arc<__LsWalSync>> {
        self.__ls_wal_sync()
    }

    pub fn __ls_start_group_commit(
        &self,
        shutdown: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Option<std::thread::JoinHandle<()>> {
        let store = self.__ls_store.as_ref()?;
        if !store.sync_each {
            return None;
        }
        let sync = std::sync::Arc::clone(&store.sync);
        {
            let mut guard = sync.mutex.lock().ok()?;
            if guard.deferred || guard.alive || guard.sealed {
                return None;
            }
            guard.deferred = true;
            guard.alive = true;
        }
        let flag = std::sync::Arc::clone(shutdown);
        let worker_sync = std::sync::Arc::clone(&sync);
        match std::thread::Builder::new()
            .name("lightshade-sync".to_string())
            .spawn(move || {
                __ls_sync_loop(worker_sync, flag);
            }) {
            Ok(worker) => Some(worker),
            Err(_) => {
                if let Ok(mut guard) = sync.mutex.lock() {
                    guard.deferred = false;
                    guard.alive = false;
                }
                sync.cond.notify_all();
                None
            }
        }
    }

    fn __ls_write_batch(&mut self, operations: &[u8]) -> io::Result<()> {
        if let Some(message) = self.__ls_wal_quota_error(operations.len()) {
            return Err(io::Error::other(message));
        }
        let length = __ls_wal_record_length(operations.len())?;
        let Some(store) = self.__ls_store.as_mut() else {
            return Ok(());
        };
        let lsn = self.__ls_lsn + 1;
        let checksum = __ls_hash_tagged(1, operations);
        let mut header = [0u8; __LS_WAL_HEADER];
        header[..8].copy_from_slice(&__LS_MAGIC);
        header[8..12].copy_from_slice(&__LS_WAL_VERSION.to_le_bytes());
        header[12..20].copy_from_slice(&__LS_SCHEMA.to_le_bytes());
        header[20..24].copy_from_slice(&length.to_le_bytes());
        header[24..32].copy_from_slice(&lsn.to_le_bytes());
        header[32..40].copy_from_slice(&checksum.to_le_bytes());
        let sync_each = store.sync_each;
        let sync = std::sync::Arc::clone(&store.sync);
        let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
        if let Some(error) = guard.failed.clone() {
            return Err(io::Error::other(error));
        }
        let deferred = guard.deferred;
        guard.wal.write_all(&header)?;
        guard.wal.write_all(&[1u8])?;
        guard.wal.write_all(operations)?;
        guard.bytes = guard
            .bytes
            .saturating_add(__LS_WAL_HEADER as u64 + 1 + operations.len() as u64);
        guard.lsn = lsn;
        if deferred {
            drop(guard);
            sync.cond.notify_one();
        } else {
            if sync_each {
                guard.wal.flush()?;
                __ls_sync_file(guard.wal.get_ref(), true)?;
            }
            guard.durable = guard.durable.max(lsn);
            drop(guard);
        }
        self.__ls_lsn = lsn;
        Ok(())
    }

    pub fn commit(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if !self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no transaction is active",
            ));
        }
        self.__ls_in_transaction = false;
        let pending = std::mem::take(&mut self.__ls_effects);
        if self.__ls_store.is_none() {
            self.__ls_memory_backup = None;
            self.__ls_check_row_limit(self.__ls_row_count())?;
            return Ok(());
        }
        if pending.is_empty() {
            return Ok(());
        }
        if let Err(error) = self.__ls_check_row_limit(self.__ls_row_count()) {
            if let Err(reload_error) = self.__ls_reload() {
                self.__ls_broken = Some(reload_error.to_string());
                return Err(reload_error);
            }
            return Err(error);
        }
        if let Some(message) = self.__ls_wal_quota_error(pending.len()) {
            if let Err(error) = self.__ls_reload() {
                self.__ls_broken = Some(error.to_string());
                return Err(error);
            }
            return Err(io::Error::other(message));
        }
        match self.__ls_write_batch(&pending) {
            Ok(()) => {
                self.__ls_maybe_checkpoint();
                Ok(())
            }
            Err(error) => {
                self.__ls_broken = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn rollback(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if !self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no transaction is active",
            ));
        }
        self.__ls_in_transaction = false;
        self.__ls_effects.clear();
        if self.__ls_store.is_none() {
            let backup = self.__ls_memory_backup.take().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "no in-memory snapshot to restore")
            })?;
            let (fresh, _) = Db::__ls_read_snapshot(io::Cursor::new(backup), false)?;
            self.__ls_adopt(fresh);
            return Ok(());
        }
        self.__ls_reload()
    }

    fn __ls_reload(&mut self) -> io::Result<()> {
        let (path, snapshot_lsn, sync) = {
            let store = self.__ls_store.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "database is not durable")
            })?;
            let sync = std::sync::Arc::clone(&store.sync);
            sync.mutex
                .lock()
                .map_err(|_| __ls_wal_poisoned())?
                .wal
                .flush()?;
            (store.path.clone(), store.snapshot_lsn, sync)
        };
        let (mut fresh, snapshot_lsn) = if path.exists() {
            Db::__ls_read_snapshot(io::BufReader::new(File::open(&path)?), false)?
        } else {
            (Db::__ls_new_spilled()?, snapshot_lsn)
        };
        let wal_path = __ls_sidecar(&path, ".wal");
        let mut wal = File::open(&wal_path)?;
        let (max_lsn, valid_length) = __ls_replay_wal(&mut fresh, &mut wal, snapshot_lsn, None, false)?;
        fresh.__ls_lsn = max_lsn;
        {
            let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
            guard.wal.get_mut().set_len(valid_length)?;
            guard.wal.get_mut().seek(SeekFrom::Start(valid_length))?;
            guard.bytes = valid_length;
            guard.lsn = max_lsn;
            guard.durable = max_lsn;
        }
        sync.cond.notify_all();
        self.__ls_adopt(fresh);
        Ok(())
    }

    pub fn checkpoint(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot checkpoint during a transaction",
            ));
        }
        if self.__ls_store.is_none() {
            return Ok(());
        }
        let path = self.__ls_store.as_ref().unwrap().path.clone();
        let temporary = __ls_sidecar(&path, ".tmp");
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        {
            let mut file = io::BufWriter::new(
                __ls_private_options()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?,
            );
            self.__ls_write_snapshot(&mut file)?;
            file.flush()?;
            __ls_sync_file(file.get_ref(), false)?;
        }
        fs::rename(&temporary, &path)?;
        __ls_sync_parent(&path)?;
        let sync = std::sync::Arc::clone(&self.__ls_store.as_ref().unwrap().sync);
        {
            let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
            guard.wal.flush()?;
            guard.wal.get_mut().set_len(0)?;
            guard.wal.get_mut().seek(SeekFrom::Start(0))?;
            __ls_sync_file(guard.wal.get_mut(), false)?;
            guard.bytes = 0;
            guard.durable = self.__ls_lsn;
        }
        sync.cond.notify_all();
        let store = self.__ls_store.as_mut().unwrap();
        store.snapshot_lsn = self.__ls_lsn;
        store.checkpoint_at = __ls_checkpoint_threshold();
        Ok(())
    }

    fn __ls_maybe_checkpoint(&mut self) {
        if self.__ls_in_transaction {
            return;
        }
        let threshold = match self.__ls_store.as_ref() {
            Some(store) => store.checkpoint_at,
            None => return,
        };
        if threshold == u64::MAX {
            return;
        }
        let current = match self.__ls_store.as_ref() {
            Some(store) => {
                let sync = std::sync::Arc::clone(&store.sync);
                sync.mutex.lock().map(|guard| guard.bytes).unwrap_or(0)
            }
            None => return,
        };
        if current < threshold {
            return;
        }
        if self.checkpoint().is_err() {
            if let Some(store) = self.__ls_store.as_mut() {
                store.checkpoint_at = current.saturating_mul(2).max(store.checkpoint_at);
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot flush during a transaction",
            ));
        }
        let sync = match self.__ls_store.as_ref() {
            Some(store) => std::sync::Arc::clone(&store.sync),
            None => return Ok(()),
        };
        let result = (|| {
            let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
            guard.wal.flush()?;
            __ls_sync_file(guard.wal.get_ref(), true)?;
            guard.durable = guard.durable.max(guard.lsn);
            Ok::<(), io::Error>(())
        })();
        sync.cond.notify_all();
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.__ls_broken = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn __ls_flush_wal(&self) -> io::Result<u64> {
        self.__ls_ready()?;
        let sync = match self.__ls_store.as_ref() {
            Some(store) => std::sync::Arc::clone(&store.sync),
            None => return Ok(self.__ls_lsn),
        };
        let lsn = (|| {
            let mut guard = sync.mutex.lock().map_err(|_| __ls_wal_poisoned())?;
            guard.wal.flush()?;
            __ls_sync_file(guard.wal.get_ref(), true)?;
            let lsn = guard.lsn;
            guard.durable = guard.durable.max(lsn);
            Ok::<u64, io::Error>(lsn)
        })()?;
        sync.cond.notify_all();
        Ok(lsn)
    }

    pub fn close(mut self) -> io::Result<()> {
        self.__ls_ready()?;
        if self.__ls_in_transaction {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot close during a transaction",
            ));
        }
        let needs_checkpoint = match &self.__ls_store {
            Some(store) => self.__ls_lsn > store.snapshot_lsn || !store.path.exists(),
            None => false,
        };
        if needs_checkpoint {
            self.checkpoint()?;
        }
        if let Some(store) = self.__ls_store.as_ref() {
            let sync = std::sync::Arc::clone(&store.sync);
            if let Ok(mut guard) = sync.mutex.lock() {
                guard.sealed = true;
            }
            sync.cond.notify_all();
        }
        self.__ls_store = None;
        Ok(())
    }
}
"#;
