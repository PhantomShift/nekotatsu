extern crate prost_build;

fn main() {
    std::env::set_var("PROTOC", "/usr/bin/protoc");

    if let Err(e) = prost_build::compile_protos(&["src/neko.proto"],&["src/"]) {
        println!("cargo:warning=Error occured when compiling neko.proto; did you generate it with proto_gen?");
        println!("cargo:warning={}", e.to_string());
    }
}