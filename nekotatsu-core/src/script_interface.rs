use std::{collections::HashMap, path::Path};

use mlua::{self, AsChunk, Function};
use thiserror::Error;

const CORRECT_RELATIVE_URL: &str = "correct_relative_url";
const CORRECT_PUBLIC_URL: &str = "correct_public_url";
const CORRECT_MANGA_IDENTIFIER: &str = "correct_manga_identifier";
const CORRECT_CHAPTER_IDENTIFIER: &str = "correct_chapter_identifier";

const REQUIRED_FUNCTIONS: &[&str] = &[
    CORRECT_RELATIVE_URL,
    CORRECT_PUBLIC_URL,
    CORRECT_MANGA_IDENTIFIER,
    CORRECT_CHAPTER_IDENTIFIER,
];

#[derive(Error, Debug)]
pub enum Error {
    #[error("error loading script")]
    LoadError(#[from] std::io::Error),
    #[error("script is not complete: {0}")]
    IncompleteError(String),
    #[error("error running script")]
    RuntimeError(#[from] mlua::Error),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct ScriptRuntime {
    pub lua: mlua::Lua,
    functions: HashMap<String, Function>,
}

impl ScriptRuntime {
    pub fn from_chunk<'a, T: AsChunk<'a> + Clone>(chunk: T) -> Result<Self> {
        let lua = mlua::Lua::new();

        lua.load(chunk).exec()?;

        let functions = ScriptRuntime::get_functions(&lua)?;

        return Ok(Self { lua, functions });
    }

    pub fn create(script_path: &Path) -> Result<Self> {
        let script_file = std::fs::read(script_path)?;
        return Self::from_chunk(script_file);
    }

    fn get_functions(lua: &mlua::Lua) -> Result<HashMap<String, Function>> {
        let mut functions = HashMap::new();

        for &func_name in REQUIRED_FUNCTIONS {
            if let Some(function) = lua.globals().get::<Option<Function>>(func_name)? {
                functions.insert(func_name.to_string(), function);
            } else {
                return Err(Error::IncompleteError(format!(
                    "Missing function '{}'",
                    func_name
                )));
            }
        }

        Ok(functions)
    }

    pub fn correct_relative_url(
        &self,
        source_name: &str,
        domain: &str,
        url: &str,
    ) -> Result<String> {
        self.functions[CORRECT_RELATIVE_URL]
            .call::<String>((source_name, domain, url))
            .map_err(Error::RuntimeError)
    }

    pub fn correct_public_url(&self, source_name: &str, domain: &str, url: &str) -> Result<String> {
        self.functions[CORRECT_PUBLIC_URL]
            .call::<String>((source_name, domain, url))
            .map_err(Error::RuntimeError)
    }

    pub fn correct_manga_identifier(&self, source_name: &str, current: &str) -> Result<String> {
        self.functions[CORRECT_MANGA_IDENTIFIER]
            .call::<String>((source_name, current))
            .map_err(Error::RuntimeError)
    }

    pub fn correct_chapter_identifier(&self, source_name: &str, current: &str) -> Result<String> {
        self.functions[CORRECT_CHAPTER_IDENTIFIER]
            .call::<String>((source_name, current))
            .map_err(Error::RuntimeError)
    }
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        static CHUNK: &str = include_str!("correction.luau");
        let lua = mlua::Lua::new();
        lua.load(CHUNK)
            .exec()
            .expect("default implementation should be valid");
        let functions = ScriptRuntime::get_functions(&lua)
            .expect("default implementation should have all necessary functions");
        Self { lua, functions }
    }
}

#[test]
fn lua_test() -> Result<()> {
    let runtime = ScriptRuntime::create(std::path::Path::new("./src/correction.luau"))?;
    let url = runtime.correct_relative_url("DANKE", "danke.moe", "lock-on")?;
    let public_url = runtime.correct_public_url("DANKE", "danke.moe", "lock-on")?;
    println!("URL: {url}");
    println!("Public URL: {public_url}");
    Ok(())
}
