fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    unsafe { std::env::set_var("PROTOC", protoc) };
    tonic_prost_build::compile_protos("../../proto/file_browser.proto")?;
    println!("cargo:rerun-if-changed=../../proto/file_browser.proto");
    Ok(())
}
