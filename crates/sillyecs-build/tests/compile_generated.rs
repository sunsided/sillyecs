//! Integration test for issue #39: render generated code into a synthetic
//! fixture crate under the workspace `target/` directory and run `cargo test`
//! to make sure the templates produce code that actually compiles. The
//! existing `build.rs` tests only string-grep the rendered output, so bugs
//! like #36 (invalid Rust in parameter lists) and #37 (missing trait bound on
//! `handle_commands`) sneak past them.
//!
//! Each fixture under `tests/fixtures/<name>/` is a pair of files:
//! - `ecs.yaml` - the YAML input to `EcsCode::generate`
//! - `user.rs`  - hand-written user-side stubs (component data, system data,
//!   `Apply<X>System` impls, `WorldCommandQueue` impl, `EntityLocationMap`
//!   alias).
//! - `runtime.rs` (optional) - `#[test]`s against the generated world, compiled
//!   as a `#[cfg(test)]` module so they exercise behavior, not just types.
//!
//! The test renders the four template outputs into the fixture crate at
//! `target/sillyecs-compile-fixtures/<name>/` (a stable workspace path, not a
//! system tempdir, so cargo's incremental cache survives across runs), then
//! shells out to `cargo test` against that crate. A non-zero exit prints the
//! captured stderr and leaves the fixture directory on disk for inspection.

use sillyecs_build::EcsCode;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Path to the `sillyecs` runtime crate, used as a `path =` dependency from the
/// generated fixture crate. Resolves at compile time against this test crate's
/// manifest dir so the test works no matter where `cargo` is invoked from.
const SILLYECS_RUNTIME_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../sillyecs");

/// Marks the start of a fixture's `paths:` block; [`Layout::BareNames`] drops everything from here.
const PATHS_MARKER: &str = "# --- paths";

/// How the fixture crate wires the generated code to the user code. In both layouts the user
/// code is a sibling module of the generated code, as in a real game, so it can only use the
/// generated items' public API.
#[derive(Clone, Copy)]
enum Layout {
    /// The YAML's `paths` block is used and the generated module imports nothing.
    Paths,
    /// The `paths` block is stripped, so the generated code names the user types by their bare
    /// names, and the generated module glob-imports them (the setup before `paths` existed).
    BareNames,
}

#[test]
fn full_coverage_fixture_compiles() {
    run_fixture("full_coverage", Layout::Paths);
}

#[test]
fn full_coverage_fixture_compiles_with_bare_names() {
    run_fixture("full_coverage", Layout::BareNames);
}

fn run_fixture(fixture_name: &str, layout: Layout) {
    let fixture_dir = PathBuf::from(FIXTURE_ROOT).join(fixture_name);
    let yaml_path = fixture_dir.join("ecs.yaml");
    let user_path = fixture_dir.join("user.rs");

    let yaml = fs::read(&yaml_path).unwrap_or_else(|e| panic!("read {}: {e}", yaml_path.display()));
    let user_rs = fs::read_to_string(&user_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", user_path.display()));
    let runtime_rs = fs::read_to_string(fixture_dir.join("runtime.rs")).unwrap_or_default();

    let (yaml, crate_name, ecs_imports) = match layout {
        Layout::Paths => (&yaml[..], fixture_name.to_string(), ""),
        Layout::BareNames => {
            let text = std::str::from_utf8(&yaml).expect("fixture YAML is UTF-8");
            let end = text
                .find(PATHS_MARKER)
                .unwrap_or_else(|| panic!("{fixture_name}/ecs.yaml has no `{PATHS_MARKER}` line"));
            (
                &yaml[..end],
                format!("{fixture_name}_bare_names"),
                "    #[allow(clippy::wildcard_imports)]\n    use crate::user::*;\n",
            )
        }
    };

    let code = EcsCode::generate(BufReader::new(yaml))
        .unwrap_or_else(|e| panic!("EcsCode::generate failed for {crate_name}: {e:?}"));

    // Stable, per-fixture workspace location so cargo's incremental cache
    // survives across test runs. Cleaned and rewritten every invocation so
    // stale state from an earlier failure can't poison a fresh run.
    let workspace_target = workspace_target_dir();
    let crate_dir = workspace_target
        .join("sillyecs-compile-fixtures")
        .join(&crate_name);
    let src_dir = crate_dir.join("src");
    let generated_dir = src_dir.join("generated");

    // Wipe the fixture crate before writing it. Silently ignoring a failed
    // deletion would let stale files from a previous run survive into the
    // next `cargo test`, which could mask a regression by compiling the old
    // state. Propagate the error so the test fails loudly instead.
    if crate_dir.exists() {
        fs::remove_dir_all(&crate_dir)
            .unwrap_or_else(|e| panic!("clear fixture dir {}: {e}", crate_dir.display()));
    }
    fs::create_dir_all(&generated_dir).expect("create fixture crate dir");

    fs::write(generated_dir.join("components_gen.rs"), &code.components).unwrap();
    fs::write(generated_dir.join("archetypes_gen.rs"), &code.archetypes).unwrap();
    fs::write(generated_dir.join("systems_gen.rs"), &code.systems).unwrap();
    fs::write(generated_dir.join("world_gen.rs"), &code.world).unwrap();

    fs::write(src_dir.join("user.rs"), &user_rs).unwrap();
    fs::write(src_dir.join("runtime.rs"), &runtime_rs).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        LIB_RS.replace("    // ECS_IMPORTS\n", ecs_imports),
    )
    .unwrap();
    fs::write(crate_dir.join("Cargo.toml"), cargo_toml(&crate_name)).unwrap();

    let target_dir = workspace_target.join("sillyecs-compile-fixtures-target");

    // `cargo test` builds and runs the runtime tests, `clippy` and `doc` add their lints. The
    // fixture's `lib.rs` denies warnings with the stricter lint levels user crates commonly set,
    // so any of these fails if a lint fires in the generated code (issue #65).
    let steps: [&[&str]; 3] = [
        &["test", "--quiet"],
        &["clippy", "--quiet", "--all-targets"],
        &["doc", "--quiet", "--no-deps", "--document-private-items"],
    ];
    for args in steps {
        let output = Command::new(env!("CARGO"))
            .args(args)
            .arg("--manifest-path")
            .arg(crate_dir.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir)
            // Inherit RUSTFLAGS / RUSTC etc. from the parent so the fixture builds
            // with the same toolchain the test runner is using.
            .output()
            .unwrap_or_else(|e| panic!("spawn cargo {}: {e}", args[0]));

        if !output.status.success() {
            panic!(
                "`cargo {}` failed for the generated code from fixture `{crate_name}`.\n\
                 crate at: {}\n\
                 --- stdout ---\n{}\n--- stderr ---\n{}",
                args.join(" "),
                crate_dir.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }
}

fn workspace_target_dir() -> PathBuf {
    // Match cargo's resolution: $CARGO_TARGET_DIR wins, otherwise the workspace
    // root's `target/`. The test crate's manifest dir is
    // `<workspace>/crates/sillyecs-build`, so the workspace root is two levels up.
    if let Ok(custom) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(custom);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root above sillyecs-build")
        .join("target")
}

fn cargo_toml(fixture_name: &str) -> String {
    format!(
        r#"[package]
name = "sillyecs-compile-fixture-{fixture_name}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
path = "src/lib.rs"

[dependencies]
sillyecs = {{ path = "{path}" }}

[workspace]
"#,
        fixture_name = fixture_name,
        path = SILLYECS_RUNTIME_PATH.replace('\\', "/"),
    )
}

const LIB_RS: &str = r#"//! Auto-generated fixture crate. See `compile_generated.rs` in `sillyecs-build`.
//!
//! The generated code must not need any allows from its user, so the lint levels here are
//! deliberately strict and apply to the generated files unchanged. The user code is a sibling
//! module of the generated code, as in a real game, and opts out of the lints on its own.
#![deny(warnings)]
#![warn(
    unsafe_code,
    unused_qualifications,
    elided_lifetimes_in_paths,
    missing_docs,
    clippy::pedantic
)]

mod ecs {
    // ECS_IMPORTS
    include!("generated/components_gen.rs");
    include!("generated/archetypes_gen.rs");
    include!("generated/systems_gen.rs");
    include!("generated/world_gen.rs");
}

#[allow(
    dead_code,
    unused_imports,
    unused_qualifications,
    missing_docs,
    clippy::all,
    clippy::pedantic
)]
mod user {
    use crate::ecs::*;
    include!("user.rs");

    #[cfg(test)]
    mod runtime {
        include!("runtime.rs");
    }
}

pub use user::smoke;
"#;
