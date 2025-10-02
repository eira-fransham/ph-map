use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use std::hash::{Hash, Hasher};

fn make_kvs() -> impl Iterator<Item = (String, String)> {
    const SIZE: usize = 2048;

    (0..SIZE).map(|i| {
        let mut hasher = std::hash::DefaultHasher::default();
        i.hash(&mut hasher);
        let hash = hasher.finish();

        let hash_lo = hash as u32;
        let hash_hi = hash >> 32u32;

        let wrapped_hash = hash as u8;

        (
            format!("{hash_lo}-test-key-{hash_hi}"),
            format!("\"test-val-{wrapped_hash}\""),
        )
    })
}

fn main() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("codegen.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());

    let kvs = make_kvs();

    let mut map_gen = phf_codegen::Map::new();

    for (key, value) in kvs {
        map_gen.entry(key, value);
    }

    writeln!(
        &mut file,
        "pub const TEST_PHF_MAP: ::phf::Map<&'static str, &'static str> = {};",
        map_gen.build()
    )
    .unwrap();
}
