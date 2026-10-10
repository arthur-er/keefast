//! What a hostile Plugin can't do: run forever, take all the memory, or reach anything outside
//! its sandbox. Each is a hand-written component of the `plugin` world whose hooks misbehave.

use std::time::{Duration, Instant};

use keefast_plugins::{EntryField, PluginError, PluginHost};

/// A manifest for a test Plugin owning one field, `Thing`.
const MANIFEST: &str = r#"{
  "id": "example.hostile",
  "name": "Hostile",
  "version": "1.0.0",
  "apiVersion": 1,
  "capabilities": ["readProtected"],
  "contributions": { "facets": [{
    "id": "thing", "title": "Thing", "detect": { "fields": ["Thing"] },
    "fields": [{ "key": "thing", "name": "Thing", "label": "Thing", "kind": "text" }]
  }] }
}"#;

/// A component of the `plugin` world whose three hooks all run `body`, with `imports` added.
fn component(body: &str, imports: &str) -> Vec<u8> {
    let wat = format!(
        r#"(component
  (type $types (instance
    (type (;0;) (option string))
    (type (;1;) (record (field "key" string) (field "value" 0)))
    (export (;2;) "field" (type (eq 1)))
    (type (;3;) (record (field "key" string) (field "text" string)))
    (export (;4;) "field-text" (type (eq 3)))
    (type (;5;) (list 4))
    (type (;6;) (record (field "label" 0) (field "fields" 5)))
    (export (;7;) "derived" (type (eq 6)))
    (type (;8;) (enum "warning" "error"))
    (export (;9;) "severity" (type (eq 8)))
    (type (;10;) (record (field "severity" 9) (field "message" string)))
    (export (;11;) "problem" (type (eq 10)))
    (type (;12;) (record (field "year" u16) (field "month" u8)))
    (export (;13;) "month-year" (type (eq 12)))))
  (import "keefast:plugin/types@1.0.0" (instance $t (type $types)))
  (alias export $t "field" (type $field))
  (alias export $t "derived" (type $derived))
  (alias export $t "problem" (type $problem))
  (alias export $t "month-year" (type $month-year))
  {imports}
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 1024)
    (func (export "format") (param i32 i32 i32 i32 i32 i32) (result i32) {body})
    (func (export "validate") (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32) {body})
    (func (export "derive") (param i32 i32 i32 i32) (result i32) {body}))
  (core instance $main (instantiate $m))
  (alias core export $main "memory" (core memory $memory))
  (alias core export $main "realloc" (core func $realloc))
  (core func $format (alias core export $main "format"))
  (core func $validate (alias core export $main "validate"))
  (core func $derive (alias core export $main "derive"))
  (func (export "format") (param "facet" string) (param "key" string) (param "value" string) (result string)
    (canon lift (core func $format) (memory $memory) (realloc $realloc)))
  (type $maybe-problem (option $problem))
  (func (export "validate") (param "facet" string) (param "key" string) (param "value" string) (param "today" $month-year) (result $maybe-problem)
    (canon lift (core func $validate) (memory $memory) (realloc $realloc)))
  (type $fields (list $field))
  (func (export "derive") (param "facet" string) (param "fields" $fields) (result $derived)
    (canon lift (core func $derive) (memory $memory) (realloc $realloc)))
)"#
    );
    wat::parse_str(wat).expect("the test component is valid")
}

fn host_with(component: &[u8]) -> PluginHost {
    let mut host = PluginHost::new();
    host.load(MANIFEST, component).unwrap();
    host
}

const THING: [EntryField; 1] = [EntryField {
    name: "Thing",
    value: "secret",
    protected: true,
}];

#[test]
fn a_plugin_that_never_returns_runs_out_of_fuel() {
    let host = host_with(&component("(loop $l (br $l)) unreachable", ""));
    let started = Instant::now();
    assert!(matches!(
        host.format("example.hostile", "thing", "thing", "x"),
        Err(PluginError::Hook(_))
    ));
    // Its facet is still found, without a label.
    let found = host.facets(&THING);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, None);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_plugin_cant_grow_its_memory_past_the_cap() {
    // 1000 pages are 64 MiB, past the 16 MiB cap.
    let host = host_with(&component(
        "(drop (memory.grow (i32.const 1000))) i32.const 0",
        "",
    ));
    let error = host
        .format("example.hostile", "thing", "thing", "x")
        .unwrap_err();
    assert!(matches!(error, PluginError::Hook(_)), "{error}");
    // A failed call doesn't leave the Plugin broken for good: it fails the same way again.
    assert!(
        host.format("example.hostile", "thing", "thing", "x")
            .is_err()
    );
}

#[test]
fn a_plugin_importing_anything_doesnt_load() {
    let clock =
        r#"(import "wasi:clocks/wall-clock@0.2.0" (instance (export "now" (func (result u64)))))"#;
    let mut host = PluginHost::new();
    let result = host.load(MANIFEST, &component("i32.const 0", clock));
    assert!(
        matches!(result, Err(PluginError::Component(_))),
        "{result:?}"
    );
}

#[test]
fn bytes_that_arent_a_component_dont_load() {
    let mut host = PluginHost::new();
    assert!(matches!(
        host.load(MANIFEST, b"not wasm"),
        Err(PluginError::Component(_))
    ));
}
