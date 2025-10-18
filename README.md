# `ph-map`

Tests to use perfect hashing to make faster maps/sets at the cost of construction time.

```
running 7 tests
test bench::bench_hashbrown_get         ... bench:          13.34 ns/iter (+/- 0.22)
test bench::bench_hashbrown_no_hash_get ... bench:           5.93 ns/iter (+/- 0.06)
test bench::bench_hashmap_get           ... bench:          15.39 ns/iter (+/- 0.36)
test bench::bench_phmap_get             ... bench:          12.09 ns/iter (+/- 0.37)
test bench::bench_phstrmap_get          ... bench:          11.49 ns/iter (+/- 0.44)

test result: ok. 0 passed; 0 failed; 2 ignored; 5 measured; 0 filtered out; finished in 1.66s
```

> All tests for 8192 elements, but benchmarks show relatively consistent improvements
> no matter the number of elements. Benchmarks all use `--features benches,gxhash`.
> `PhMap` is slower when using the hashing implementation that is enabled
> by default (`rapidhash`), although `PhStrMap` is still faster. All hashmaps use the
> same hashing algorithm, either `gxhash` or `rapidhash` depending on features.

### Benchmark descriptions

- `bench_hashbrown_get`: `hashbrown::HashMap` with `gxhash`
- `bench_hashbrown_no_hash_get`: `hashbrown::HashMap` but accessed using precalculated
  hashes. This is testing what would happen if the map was keyed with a "hash-carrying"
  type.
- `bench_hashmap_get`: `std::collections::HashMap` with `gxhash`
- `bench_phmap_get`: `PhMap<String, String, str>` with `gxhash`
- `bench_phstrmap_get`: `PhStrMap<String>`. This type precalculates the first all-uncommon substring within
  the key, and will only check that substring upon retrieval. Currently, this will return false-positives
  if an input string is not in the set, but where the substring with the precalculated range matches a string
  in the set, although it cannot cause undefined behaviour in this case.
