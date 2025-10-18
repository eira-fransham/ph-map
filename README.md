# `ph-map`

Tests to use perfect hashing to make faster maps/sets given unlimited construction time.

```
running 7 tests
test test::find_smallest_uncommon_range ... ignored
test test::it_works ... ignored
test bench::bench_hashbrown_get         ... bench:          13.34 ns/iter (+/- 0.22)
test bench::bench_hashbrown_no_hash_get ... bench:           5.93 ns/iter (+/- 0.06)
test bench::bench_hashmap_get           ... bench:          15.39 ns/iter (+/- 0.36)
test bench::bench_phmap_get             ... bench:          12.09 ns/iter (+/- 0.37)
test bench::bench_phstrmap_get          ... bench:          11.49 ns/iter (+/- 0.44)

test result: ok. 0 passed; 0 failed; 2 ignored; 5 measured; 0 filtered out; finished in 1.66s
```
