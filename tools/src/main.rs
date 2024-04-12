use clap::{Parser, Subcommand};
use regex::Regex;
use lazy_static::lazy_static;

/// Tools for working with proto files
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    commands: Commands
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Compile proto files into Rust files
    Compile,
    
    /// Generate proto files from Kotlin definitions
    #[command(visible_alias = "gen")]
    Generate {
        input: std::path::PathBuf,

        #[arg(default_value_t = format!("{}/../src/neko.proto", env!("CARGO_MANIFEST_DIR")))]
        output: String,
    }
}

fn compile_proto() {
    let src_dir = format!("{}/../src", env!("CARGO_MANIFEST_DIR"));

    std::env::set_var("OUT_DIR", &src_dir);
    prost_build::compile_protos(&[src_dir.clone() + "/neko.proto"], &[&src_dir]).unwrap();
}

fn generate_proto(input: std::path::PathBuf, output: String) {
    lazy_static!(
        static ref CLASS_REGEX: Regex = Regex::new(r"@Serializable\s?(?:data )?class (?P<class_name>\w+)").unwrap();
        static ref FIELD_REGEX: Regex = Regex::new(r"@ProtoNumber\((?P<tag_number>\d+)\)\s*(?:val|var) (?P<name>[a-zA-Z_][a-zA-Z_0-9]*)\s*:\s(?P<type>\w+)(?:<(?P<list_type>\w+)>)?(?P<optional>\?)?").unwrap();
        static ref GENERAL_REGEX: Regex = Regex::new(r"@Serializable\s?(?:data )?class (?P<class_name>\w+)\((?:\s*(?:\/\/.+)|(?:\s*(?:@ProtoNumber|@Deprecated|var|val).*))+").unwrap();
    );

    let dir = std::fs::read_dir(input).expect("error reading dir");
    let mut result = String::new();
    result.push_str("// Automatically generated by proto_gen\n");
    result.push_str("syntax = \"proto3\";\n\npackage neko.backup;\n\n\n");
    for entry in dir {
        if let Ok(entry) = entry {
            let read = std::fs::read_to_string(entry.path()).expect("error reading file");
            result += &format!("// {entry:?}\n");
            let mut index = 0;
            while let Some(captures) = GENERAL_REGEX.captures_at(&read, index) {
                let class_name = captures.name("class_name").unwrap().as_str();
                let matched = captures.get(0).unwrap();
                
                let fields = FIELD_REGEX.find_iter(&matched.as_str())
                    .map(|matched| {
                        let captures = FIELD_REGEX.captures(matched.as_str()).expect("should only match if captured");
                        let tag_number = captures.name("tag_number").expect("tag_number should match").as_str();
                        let name = captures.name("name").expect("name should match").as_str();
                        let var_type = captures.name("type").expect("type should match").as_str();
                        let list_type = captures.name("list_type");
                        let is_optional = captures.name("optional").is_some();
                        format!(
                            "    {rep_or_opt}{converted_type} {name} = {tag_number};\n",
                            rep_or_opt = if is_optional {
                                "optional "
                            } else if list_type.is_some() {
                                "repeated "
                            } else {
                                ""
                            },
                            converted_type = {
                                let var_type = if let Some(t) = list_type {t.as_str()} else {var_type};
                                match var_type {
                                    "String" => "string",
                                    "Int" => "int32",
                                    "Long" => "int64",
                                    "Float" => "float",
                                    "Boolean" => "bool",
                                    _ => var_type
                                }
                            }
                        )
                    })
                    .collect::<String>();
                result += &format!("message {class_name} {{\n{fields}}}\n\n");

                index = matched.end();
            }
        }
    }

    std::fs::write(output, result).expect("error writing to file");
}

fn main() {
    let args = Args::parse();

    match args.commands {
        Commands::Compile => compile_proto(),
        Commands::Generate { input, output } => generate_proto(input, output),
    }
}