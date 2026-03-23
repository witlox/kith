fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["../../proto/kith/daemon/v1/daemon.proto"],
            &["../../proto"],
        )?;
    Ok(())
}
