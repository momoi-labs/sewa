// Generates the OpAMP messages from the protos in `proto/`. protox parses
// them in Rust, so the build needs no protoc.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../proto/opamp");
    let descriptors = protox::compile(["opamp/v1/opamp.proto"], ["../../proto"])?;
    prost_build::Config::new()
        .disable_comments(["."])
        .compile_fds(descriptors)?;
    Ok(())
}
