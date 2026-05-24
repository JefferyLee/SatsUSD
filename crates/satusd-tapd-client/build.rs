fn main() {
    tonic_prost_build::configure()
        .build_server(false)
        .compile_protos(
            &[
                "proto/tapcommon.proto",
                "proto/taprootassets.proto",
                "proto/assetwallet.proto",
            ],
            &["proto"],
        )
        .expect("compile tapd protos");
}
