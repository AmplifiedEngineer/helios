// SPDX-License-Identifier: Apache-2.0

use clap::Parser;

#[derive(Parser)]
#[command(version)]
struct Cli {}

fn main() {
    Cli::parse();
}
