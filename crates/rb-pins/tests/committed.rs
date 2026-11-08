//! The configuration files at the top of the repository parse and agree with each other.

use rb_pins::pins::PinsFile;
use rb_pins::places::{Builders, Rows};
use rb_pins::sets::Sets;
use rb_pins::toolchains::{Personas, Toolchains, check};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_file_parses() {
    let root = root();
    Sets::load(&root).unwrap();
    PinsFile::load(&root)
        .unwrap()
        .expect("pins.toml is committed");
    Personas::load(&root).unwrap();
    Toolchains::load(&root).unwrap();
    Builders::load(&root).unwrap();
    Rows::load(&root).unwrap();
}

#[test]
fn every_pin_has_a_persona_that_agrees_with_its_reference_and_tree() {
    let root = root();
    let pins = PinsFile::load(&root).unwrap().unwrap();
    let lines = check(
        &pins.pin,
        &Personas::load(&root).unwrap(),
        &Toolchains::load(&root).unwrap(),
        "x86_64",
    );
    assert_eq!(lines.len(), pins.pin.len());
    for line in &lines {
        assert!(line.review.is_empty(), "{}: {:?}", line.pin, line.review);
    }
}

#[test]
fn builders_and_rows_name_known_toolchains_and_machines() {
    let root = root();
    let toolchains = Toolchains::load(&root).unwrap();
    let builders = Builders::load(&root).unwrap();
    for (name, builder) in &builders.builder {
        for t in &builder.toolchains {
            assert!(toolchains.toolchain.contains_key(t), "{name} installs {t}");
        }
    }
    for (name, row) in &Rows::load(&root).unwrap().row {
        for m in &row.test_on {
            assert!(builders.machine.contains_key(m), "{name} tests on {m}");
        }
    }
}
