//! End-to-end checks over the shipped `examples/*.diorama.yaml` and snapshot tests of the public outputs.

use std::fs;
use std::path::{Path, PathBuf};

use dioramic_spec::{Model, SourceFormat, SpecV1, json_schema, load, normalize, parse};

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_paths() -> Vec<PathBuf> {
    let entries = fs::read_dir(examples_dir()).unwrap_or_else(|error| panic!("examples dir: {error}"));
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".diorama.yaml"))
        .collect();
    paths.sort();
    paths
}

fn pretty_json(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|error| panic!("{error}"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn load_model(name: &str) -> Model {
    let source = read(&examples_dir().join(name));
    load(&source, SourceFormat::Yaml).unwrap_or_else(|error| panic!("{name}:\n{error}")).model
}

#[test]
fn every_example_loads_without_errors() {
    let paths = example_paths();
    assert!(paths.len() >= 6, "expected the shipped examples, found {paths:?}");
    for path in paths {
        let source = read(&path);
        if let Err(diagnostics) = load(&source, SourceFormat::Yaml) {
            panic!("{}:\n{diagnostics}", path.display());
        }
    }
}

#[test]
fn yaml_and_json_of_the_same_spec_give_the_same_model() {
    for path in example_paths() {
        let source = read(&path);
        let spec = parse(&source, SourceFormat::Yaml).unwrap_or_else(|error| panic!("{error}"));
        let json = serde_json::to_string(&spec).unwrap_or_else(|error| panic!("{error}"));
        let from_json: SpecV1 = parse(&json, SourceFormat::Json).unwrap_or_else(|error| panic!("{error}"));
        let yaml_model = normalize(spec).unwrap_or_else(|error| panic!("{error}"));
        let json_model = normalize(from_json).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(yaml_model, json_model, "{}", path.display());
    }
}

#[test]
fn checkout_model_snapshot() {
    insta::assert_snapshot!("checkout_model", pretty_json(&load_model("checkout.diorama.yaml")));
}

#[test]
fn bank_branch_model_snapshot() {
    insta::assert_snapshot!("bank_branch_model", pretty_json(&load_model("bank-branch.diorama.yaml")));
}

#[test]
fn json_schema_snapshot() {
    let schema = serde_json::to_value(json_schema()).unwrap_or_default();
    assert_eq!(schema["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert!(schema["required"].as_array().is_some_and(|required| required.contains(&"nodes".into())));
    insta::assert_snapshot!("spec_v1_schema", pretty_json(&schema));
}
