fn main() {
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["proto/price_oracle.proto"], &["proto"])
        .expect("compile price_oracle proto");
}
