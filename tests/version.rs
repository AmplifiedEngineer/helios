// SPDX-License-Identifier: Apache-2.0

//! Guards the version flags, which the Homebrew formula test runs.

use std::process::Command;

const VERSION_FLAGS: [&str; 2] = ["--version", "-V"];

fn expected_output() -> String {
    format!("helios {}\n", env!("CARGO_PKG_VERSION"))
}

#[test]
fn version_flags_print_the_name_and_the_cargo_version() {
    for flag in VERSION_FLAGS {
        // Arrange
        let mut command = Command::new(env!("CARGO_BIN_EXE_helios"));
        command.arg(flag);

        // Act
        let output = command.output().expect("helios should run");

        // Assert
        assert!(output.status.success(), "{flag} should exit with success");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_output(),
            "{flag} should print the name and version"
        );
    }
}
