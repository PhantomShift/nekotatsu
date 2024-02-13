extern crate prost_build;
extern crate protobuf_src;

fn main() {
    std::env::set_var("PROTOC", protobuf_src::protoc());

    if let Err(e) = prost_build::compile_protos(&["src/neko.proto"],&["src/"]) {
        println!("cargo:warning=Error occured when compiling neko.proto; did you generate it with proto_gen?");
        println!("cargo:warning={}", e.to_string());
    }
}