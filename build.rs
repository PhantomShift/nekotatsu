extern crate prost_build;

fn main() {
    if let Err(e) = prost_build::compile_protos(&["src/neko.proto"],&["src/"]) {
        println!("cargo:warning=Error occured when compiling neko.proto; did you generate it with proto_gen?");
        println!("cargo:warning={}", e.to_string());
    }
}