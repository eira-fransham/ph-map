#![cfg_attr(feature = "benches", feature(test))]

use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::Range;
use std::{hash::Hash, marker::PhantomData};

use bitvec::bitvec;
use itertools::Itertools;
use ph::seeds::BitsFast;
use ph::{BuildDefaultSeededHasher, BuildSeededHasher};

type Function = ph::phast::Perfect<BitsFast, ph::phast::SeedOnly, BuildDefaultSeededHasher>;
pub struct PhMap<KOwned, V, KRef = KOwned>
where
    KRef: ?Sized + Hash,
    KOwned: AsRef<KRef>,
{
    keys: Vec<KOwned>,
    top_level_hashes: Vec<u64>,
    values: Vec<MaybeUninit<V>>,
    to_index: Function,
    _phantom: PhantomData<fn(&KRef)>,
}

pub struct PhStrMap<V> {
    range: Range<usize>,
    inner_map: ManuallyDrop<PhMap<Vec<u8>, V, [u8]>>,
}

impl<V> Default for PhStrMap<V> {
    fn default() -> Self {
        Self {
            range: 0..0,
            inner_map: Default::default(),
        }
    }
}

impl<V> PhStrMap<V> {
    pub fn insert(&mut self, key: String, value: V) {
        self.extend(std::iter::once((key, value)))
    }

    pub fn extend<KV>(&mut self, kv: KV)
    where
        KV: IntoIterator<Item = (String, V)>,
    {
        let mut kvs: Vec<(Vec<u8>, V)> = kv.into_iter().map(|(k, v)| (k.into_bytes(), v)).collect();
        let range = smallest_uncommon_range(kvs.iter().map(|(k, _)| &**k));

        kvs.iter_mut().for_each(|(k, _)| {
            let substring = k[range.clone()].to_owned();
            *k = substring;
        });

        assert!(range == self.range || self.range.is_empty());

        self.range = range;

        assert!(kvs.iter().map(|(k, _)| &**k).all_unique());

        self.inner_map.extend(kvs);
    }

    pub fn get<K>(&self, key: &K) -> Option<&V>
    where
        K: ?Sized + AsRef<str>,
    {
        self.inner_map
            .get(&key.as_ref().as_bytes()[self.range.clone()])
    }

    /// # Safety
    /// `key` must be in the map.
    pub unsafe fn get_unchecked<K>(&self, key: &K) -> &V
    where
        K: ?Sized + AsRef<str>,
    {
        unsafe {
            self.inner_map
                .get_unchecked(&key.as_ref().as_bytes()[self.range.clone()])
        }
    }

    pub fn get_mut<K>(&mut self, key: &K) -> Option<&mut V>
    where
        K: ?Sized + AsRef<str>,
    {
        self.inner_map
            .get_mut(&key.as_ref().as_bytes()[self.range.clone()])
    }

    /// # Safety
    /// `key` must be in the map.
    pub unsafe fn get_unchecked_mut<K>(&mut self, key: &K) -> &mut V
    where
        K: ?Sized + AsRef<str>,
    {
        unsafe {
            self.inner_map
                .get_unchecked_mut(&key.as_ref().as_bytes()[self.range.clone()])
        }
    }
}

impl<KOwned, V, KRef> Drop for PhMap<KOwned, V, KRef>
where
    KRef: ?Sized + Hash,
    KOwned: AsRef<KRef>,
{
    fn drop(&mut self) {
        let mut dropped = bitvec![0; self.values.len()];

        for key in &self.keys {
            // TODO: This assumes that the `Hash` implementation for `KRef` is well-behaved,
            //       but does not cause unsafety if this is not the case.
            let hash = self.to_index.hasher().hash_one(key.as_ref(), 0);
            let Some(idx) = self.to_index.get_with_top_level_hash(&key.as_ref(), hash) else {
                continue;
            };
            if !unsafe { *dropped.get_unchecked(idx) } {
                let ptr = unsafe { self.values.get_unchecked_mut(idx).as_mut_ptr() };

                unsafe {
                    std::ptr::drop_in_place(ptr);
                    dropped.set_unchecked(idx, true);
                }
            }
        }
    }
}

impl<KOwned, V, KRef> Default for PhMap<KOwned, V, KRef>
where
    KRef: ?Sized + Hash,
    KOwned: AsRef<KRef>,
{
    fn default() -> Self {
        let keys: &[&KRef] = &[];
        let to_index = Function::with_slice_p_hash_sc(
            keys,
            &ph::phast::Params::new(BitsFast(0), ph::phast::bits_per_seed_to_100_bucket_size(0)),
            BuildDefaultSeededHasher::default(),
            ph::phast::SeedOnly,
        );
        Self {
            keys: vec![],
            values: vec![],
            top_level_hashes: vec![],
            to_index,
            // member_set: Set::default(),
            _phantom: PhantomData,
        }
    }
}

impl<KOwned, V, KRef> PhMap<KOwned, V, KRef>
where
    KRef: ?Sized + Hash,
    KOwned: AsRef<KRef>,
{
    pub fn insert(&mut self, key: KOwned, value: V) {
        self.extend(std::iter::once((key, value)))
    }

    pub fn extend<KV>(&mut self, kv: KV)
    where
        KV: IntoIterator<Item = (KOwned, V)>,
    {
        unsafe { self.values.set_len(0) };

        let hasher = &self.to_index.hasher();

        let (keys, values_and_key_hashes): (Vec<_>, Vec<_>) = self
            .keys
            .drain(..)
            .map(|key| {
                let value = unsafe { take_unchecked(&self.values, &self.to_index, key.as_ref()) };

                (key, value)
            })
            .chain(kv)
            .map(|(key, value)| {
                let hash = hasher.hash_one(key.as_ref(), 0);

                (key, (value, hash))
            })
            .unzip();

        let bits = keys.len().next_power_of_two().ilog(2) + 1;
        let bits_u8 = bits.try_into().unwrap();
        {
            let key_refs = keys.iter().map(|k| k.as_ref()).collect::<Vec<_>>();
            self.to_index = Function::with_vec_p_hash_sc(
                key_refs,
                &ph::phast::Params::new(
                    BitsFast(bits_u8),
                    ph::phast::bits_per_seed_to_100_bucket_size(bits_u8),
                ),
                BuildDefaultSeededHasher::default(),
                ph::phast::SeedOnly,
            );
        }

        // We need to set a minimum value above 4, as for some reason Rust doesn't reserve
        // correctly for small vectors.
        let mut max_idx = keys.len();

        self.top_level_hashes = vec![0; keys.len()];

        let all_indices_unique = keys
            .iter()
            .zip(values_and_key_hashes)
            .map(|(key, (value, hash))| {
                let idx = self
                    .to_index
                    .get_with_top_level_hash(key.as_ref(), hash)
                    .unwrap();

                max_idx = max_idx.max(idx);

                if let Some(extra_capacity) = (max_idx + 1).checked_sub(self.values.capacity()) {
                    self.values.reserve(extra_capacity);
                }

                self.top_level_hashes
                    .resize(self.top_level_hashes.len().max(max_idx + 1), 0);

                debug_assert!(self.values.capacity() > max_idx);
                debug_assert!(self.top_level_hashes.len() > max_idx);

                // Safety: The inner values are `MaybeUninit` anyway
                unsafe { self.values.set_len(max_idx + 1) };
                unsafe {
                    self.values.get_unchecked_mut(idx).write(value);
                    *self.top_level_hashes.get_unchecked_mut(idx) = hash;
                }

                idx
            })
            .all_unique();

        assert!(all_indices_unique);

        self.values.shrink_to_fit();

        self.keys = keys;
    }

    pub fn get<K>(&self, key: &K) -> Option<&V>
    where
        K: ?Sized + AsRef<KRef>,
    {
        // TODO: This assumes that the `Hash` implementation for `KRef` is well-behaved,
        //       but does not cause unsafety if this is not the case.
        let hash = self.to_index.hasher().hash_one(key.as_ref(), 0);
        let idx = self.to_index.get_with_top_level_hash(key.as_ref(), hash)?;
        if *self.top_level_hashes.get(idx)? == hash {
            Some(unsafe { self.values.get_unchecked(idx).assume_init_ref() })
        } else {
            None
        }
    }

    /// # Safety
    /// `key` must be in the map.
    pub unsafe fn get_unchecked<K>(&self, key: &K) -> &V
    where
        K: ?Sized + AsRef<KRef>,
    {
        let idx = unsafe { self.to_index.get(key.as_ref()).unwrap_unchecked() };
        unsafe { self.values.get_unchecked(idx).assume_init_ref() }
    }

    pub fn get_mut<K>(&mut self, key: &K) -> Option<&mut V>
    where
        K: ?Sized + AsRef<KRef>,
    {
        // TODO: This assumes that the `Hash` implementation for `KRef` is well-behaved,
        //       but does not cause unsafety if this is not the case.
        let hash = self.to_index.hasher().hash_one(key.as_ref(), 0);
        let idx = self.to_index.get_with_top_level_hash(key.as_ref(), hash)?;
        if *self.top_level_hashes.get(idx)? == hash {
            Some(unsafe { self.values.get_unchecked_mut(idx).assume_init_mut() })
        } else {
            None
        }
    }
    /// # Safety
    /// `key` must be in the map.
    pub unsafe fn get_unchecked_mut<K>(&mut self, key: &K) -> &mut V
    where
        K: ?Sized + AsRef<KRef>,
    {
        let idx = unsafe { self.to_index.get(key.as_ref()).unwrap_unchecked() };
        unsafe { self.values.get_unchecked_mut(idx).assume_init_mut() }
    }
}

/// # Safety
/// `to_index` must have been created with `key` as one of its keys, and `vals` must have a length
/// of at least the maxmimum value that `to_index` can return.
unsafe fn get_unchecked_uninit<'a, K, V>(
    vals: &'a [MaybeUninit<V>],
    to_index: &Function,
    key: &K,
) -> &'a MaybeUninit<V>
where
    K: ?Sized + Hash,
{
    unsafe { vals.get_unchecked(to_index.get(&key).unwrap_unchecked()) }
}

/// # Safety
/// `to_index` must have been created with `key` as one of its keys, and `vals` must have a length
/// of at least the maxmimum value that `to_index` can return.
pub unsafe fn take_unchecked<K, V>(vals: &[MaybeUninit<V>], to_index: &Function, key: &K) -> V
where
    K: ?Sized + Hash,
{
    unsafe { get_unchecked_uninit(vals, to_index, key).assume_init_read() }
}

/// `strs` must be sorted.
fn smallest_uncommon_range<'a, I>(strs: I) -> Range<usize>
where
    I: IntoIterator<Item = &'a [u8]>,
    I::IntoIter: ExactSizeIterator + Clone,
{
    let strs = strs.into_iter();
    let mut start = 0;

    loop {
        if !strs.clone().map(|i| i[start]).all_equal() {
            break;
        }

        start += 1;
    }

    let mut out = start..start + 1;
    loop {
        if strs.clone().map(|s| &s[out.clone()]).all_unique() {
            println!("Found {out:?}");
            break;
        }

        out.end += 1;
    }

    out
}

#[cfg(test)]
mod test {
    use std::hash::{Hash as _, Hasher as _};

    use super::smallest_uncommon_range;
    use crate::PhMap;

    #[test]
    fn it_works() {
        let mut hashmap: PhMap<&str, &str, str> = PhMap::default();

        let kvs = [
            ("foo1", "bar"),
            ("foo2", "baz"),
            ("foo3", "bar"),
            ("foo4", "qux"),
            ("foo5", "foobar"),
            ("foo6", "bazqux"),
        ];
        hashmap.extend(kvs.iter().copied());

        for (k, v) in kvs {
            assert_eq!(unsafe { hashmap.get_unchecked(k) }, &v);
        }
    }

    #[test]
    fn find_smallest_uncommon_range() {
        fn make_kvs() -> impl Iterator<Item = (String, String)> {
            const SIZE: usize = 4096;

            (0..SIZE).map(|i| {
                let mut hasher = std::hash::DefaultHasher::default();
                i.hash(&mut hasher);
                let hash = hasher.finish();

                let hash_lo = hash as u32;
                let hash_hi = hash >> 32;

                let wrapped_hash = hash as u8;

                (
                    format!("test-key-{hash_lo}-test-{hash_hi}"),
                    format!("test-val-{wrapped_hash}"),
                )
            })
        }

        let (ks, _vs): (Vec<_>, Vec<_>) = make_kvs().unzip();
        assert_eq!(
            smallest_uncommon_range(ks.iter().map(|k| k.as_bytes())),
            9..18,
        );
    }
}

#[cfg(all(test, feature = "benches"))]
mod bench {
    extern crate test;

    #[cfg(feature = "gxhash")]
    type DefaultBuildHasher = gxhash::GxBuildHasher;
    #[cfg(not(feature = "gxhash"))]
    type DefaultBuildHasher = rapidhash::RapidBuildHasher;

    use crate::{PhMap, PhStrMap};
    use std::{
        collections::HashMap,
        hash::{BuildHasher, Hash, Hasher},
    };

    fn make_kvs() -> impl Iterator<Item = (String, String)> {
        const SIZE: usize = 4096;

        (0..SIZE).map(|i| {
            let mut hasher = std::hash::DefaultHasher::default();
            i.hash(&mut hasher);
            let hash = hasher.finish();

            let hash_lo = hash as u32;
            let hash_hi = hash >> 32 as u32;

            let wrapped_hash = hash as u8;

            (
                format!("{hash_lo}-test-key-{hash_hi}"),
                format!("test-val-{wrapped_hash}"),
            )
        })
    }

    #[bench]
    fn bench_phmap_get(b: &mut test::Bencher) {
        let mut ph_map = PhMap::<String, String, str>::default();
        let kvs = make_kvs().collect::<Vec<_>>();
        ph_map.extend(kvs.iter().cloned());

        let mut idxs = (0..kvs.len()).cycle();

        b.iter(|| {
            let (key, value) = std::hint::black_box(&kvs[idxs.next().unwrap()]);
            assert_eq!(std::hint::black_box(ph_map.get(&key[..])), Some(value));
        })
    }

    #[bench]
    fn bench_phstrmap_get(b: &mut test::Bencher) {
        let mut ph_map = PhStrMap::<String>::default();
        let kvs = make_kvs().collect::<Vec<_>>();
        ph_map.extend(kvs.iter().cloned());

        let mut idxs = (0..kvs.len()).cycle();

        b.iter(|| {
            let (key, value) = std::hint::black_box(&kvs[idxs.next().unwrap()]);
            assert_eq!(std::hint::black_box(ph_map.get(&key[..])), Some(value));
        })
    }

    #[bench]
    fn bench_hashmap_get(b: &mut test::Bencher) {
        let mut hashmap = HashMap::<String, String, DefaultBuildHasher>::with_hasher(
            DefaultBuildHasher::default(),
        );
        let kvs = make_kvs().collect::<Vec<_>>();
        hashmap.extend(kvs.iter().cloned());

        let mut idxs = (0..kvs.len()).cycle();

        b.iter(|| {
            let (key, value) = std::hint::black_box(&kvs[idxs.next().unwrap()]);
            assert_eq!(std::hint::black_box(hashmap.get(&key[..])), Some(value));
        })
    }

    #[bench]
    fn bench_hashbrown_get(b: &mut test::Bencher) {
        let mut hashbrown =
            hashbrown::HashMap::<String, String, _>::with_hasher(DefaultBuildHasher::default());
        let kvs = make_kvs().collect::<Vec<_>>();
        hashbrown.extend(kvs.iter().cloned());

        let mut idxs = (0..kvs.len()).cycle();

        b.iter(|| {
            let (key, value) = std::hint::black_box(&kvs[idxs.next().unwrap()]);
            assert_eq!(std::hint::black_box(hashbrown.get(&key[..])), Some(value));
        })
    }

    #[bench]
    fn bench_hashbrown_no_hash_get(b: &mut test::Bencher) {
        let mut hashbrown = hashbrown::HashMap::<u64, String, _>::with_hasher(BuildIdentityHasher);
        let build_hasher = DefaultBuildHasher::default();
        let kvs = make_kvs()
            .map(|(k, v)| (build_hasher.hash_one(k), v))
            .collect::<Vec<_>>();
        hashbrown.extend(kvs.iter().cloned());

        let mut idxs = (0..kvs.len()).cycle();

        b.iter(|| {
            let (key, value) = std::hint::black_box(&kvs[idxs.next().unwrap()]);
            assert_eq!(std::hint::black_box(hashbrown.get(key)), Some(value));
        })
    }

    struct BuildIdentityHasher;

    impl BuildHasher for BuildIdentityHasher {
        type Hasher = IdentityHasher;

        // Required method
        fn build_hasher(&self) -> Self::Hasher {
            IdentityHasher(0)
        }
    }

    #[repr(transparent)]
    struct IdentityHasher(u64);

    impl Hasher for IdentityHasher {
        fn finish(&self) -> u64 {
            self.0
        }

        fn write(&mut self, bytes: &[u8]) {
            self.0 = u64::from_ne_bytes(bytes.try_into().unwrap());
        }

        fn write_u64(&mut self, i: u64) {
            self.0 = i;
        }

        fn write_u8(&mut self, i: u8) {
            self.0 = i as u64;
        }

        fn write_u16(&mut self, i: u16) {
            self.0 = i as u64;
        }

        fn write_u32(&mut self, i: u32) {
            self.0 = i as u64;
        }

        fn write_u128(&mut self, i: u128) {
            self.0 = i as u64;
        }

        fn write_usize(&mut self, i: usize) {
            self.0 = i as u64;
        }

        fn write_i8(&mut self, i: i8) {
            self.0 = i as u64;
        }

        fn write_i16(&mut self, i: i16) {
            self.0 = i as u64;
        }

        fn write_i32(&mut self, i: i32) {
            self.0 = i as u64;
        }

        fn write_i64(&mut self, i: i64) {
            self.0 = i as u64;
        }

        fn write_i128(&mut self, i: i128) {
            self.0 = i as u64;
        }

        fn write_isize(&mut self, i: isize) {
            self.0 = i as u64;
        }
    }
}
