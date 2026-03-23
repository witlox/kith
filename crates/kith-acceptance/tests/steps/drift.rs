//! Step definitions for drift-detection.feature

use cucumber::{given, then, when};
use kith_common::drift::{DriftCategory, DriftWeights};

use crate::KithWorld;

#[given(expr = "drift weights are configured as files={float}, services={float}, network={float}, packages={float}")]
fn set_drift_weights(world: &mut KithWorld, files: f64, services: f64, network: f64, packages: f64) {
    world.drift_weights = DriftWeights {
        files,
        services,
        network,
        packages,
    };
}

#[when(expr = "{int} file changes and {int} service change(s) have been detected")]
fn detect_file_and_service_changes(world: &mut KithWorld, file_count: u32, service_count: u32) {
    for _ in 0..file_count {
        world.drift_vector.increment(&DriftCategory::Files);
    }
    for _ in 0..service_count {
        world.drift_vector.increment(&DriftCategory::Services);
    }
}

#[then(expr = "the squared drift magnitude is {float}")]
fn check_magnitude(world: &mut KithWorld, expected: f64) {
    let actual = world.drift_vector.magnitude_sq(&world.drift_weights);
    assert!(
        (actual - expected).abs() < 0.01,
        "expected magnitude_sq {expected}, got {actual}"
    );
}

#[then(expr = "the drift vector shows files={float}, services={float}, network={float}, packages={float}")]
fn check_drift_vector(
    world: &mut KithWorld,
    files: f64,
    services: f64,
    network: f64,
    packages: f64,
) {
    assert!(
        (world.drift_vector.files - files).abs() < 0.01,
        "files: expected {files}, got {}",
        world.drift_vector.files
    );
    assert!(
        (world.drift_vector.services - services).abs() < 0.01,
        "services: expected {services}, got {}",
        world.drift_vector.services
    );
    assert!(
        (world.drift_vector.network - network).abs() < 0.01,
        "network: expected {network}, got {}",
        world.drift_vector.network
    );
    assert!(
        (world.drift_vector.packages - packages).abs() < 0.01,
        "packages: expected {packages}, got {}",
        world.drift_vector.packages
    );
}
