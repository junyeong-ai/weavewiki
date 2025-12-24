//! Language Detection Module
//!
//! **Single source of truth** for all language detection across the codebase.
//! All language detection MUST use this module - no duplicate implementations allowed.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use weavewiki::analyzer::parser::language::{Language, detect_language};
//!
//! // Using the enum
//! let lang = Language::from_path("src/main.rs");
//! assert_eq!(lang, Language::Rust);
//! assert_eq!(lang.highlight_str(), "rust");
//!
//! // Using the standalone function (for syntax highlighting)
//! let highlight = detect_language("src/main.rs");
//! assert_eq!(highlight, Some("rust"));
//! ```

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// =============================================================================
// Language Metadata Table - Single Source of Truth
// =============================================================================

/// Language metadata entry containing all language-specific information
struct LanguageMeta {
    /// Display name (human-readable)
    display_name: &'static str,
    /// Syntax highlighting identifier (lowercase, for markdown code blocks)
    highlight_str: &'static str,
    /// File extensions that map to this language
    extensions: &'static [&'static str],
    /// Alternative names for parsing from string
    aliases: &'static [&'static str],
    /// Whether this language has tree-sitter parser support
    has_parser: bool,
}

/// Macro to define language metadata concisely
macro_rules! lang_meta {
    ($display:literal, $highlight:literal, [$($ext:literal),*], [$($alias:literal),*], $parser:literal) => {
        LanguageMeta {
            display_name: $display,
            highlight_str: $highlight,
            extensions: &[$($ext),*],
            aliases: &[$($alias),*],
            has_parser: $parser,
        }
    };
}

impl Language {
    /// Get metadata for this language variant
    fn meta(&self) -> LanguageMeta {
        match self {
            // Systems Languages
            Language::Rust => lang_meta!("Rust", "rust", ["rs"], ["rust"], true),
            Language::Go => lang_meta!("Go", "go", ["go"], ["go", "golang"], true),
            Language::C => lang_meta!("C", "c", ["c", "h"], ["c"], true),
            Language::Cpp => lang_meta!("C++", "cpp", ["cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++"], ["cpp", "c++", "cxx"], true),
            Language::Zig => lang_meta!("Zig", "zig", ["zig"], ["zig"], false),
            Language::Nim => lang_meta!("Nim", "nim", ["nim"], ["nim"], false),

            // JVM Languages
            Language::Java => lang_meta!("Java", "java", ["java"], ["java"], true),
            Language::Kotlin => lang_meta!("Kotlin", "kotlin", ["kt", "kts"], ["kotlin", "kt"], true),
            Language::Scala => lang_meta!("Scala", "scala", ["scala", "sc"], ["scala"], false),
            Language::Groovy => lang_meta!("Groovy", "groovy", ["groovy", "gvy", "gy", "gsh"], ["groovy"], false),
            Language::Clojure => lang_meta!("Clojure", "clojure", ["clj", "cljs", "cljc", "edn"], ["clojure", "clj"], false),

            // Web Languages
            Language::TypeScript => lang_meta!("TypeScript", "typescript", ["ts", "mts", "cts"], ["typescript", "ts"], true),
            Language::JavaScript => lang_meta!("JavaScript", "javascript", ["js", "mjs", "cjs"], ["javascript", "js"], true),
            Language::Tsx => lang_meta!("TSX", "tsx", ["tsx"], ["tsx"], true),
            Language::Jsx => lang_meta!("JSX", "jsx", ["jsx"], ["jsx"], true),
            Language::Html => lang_meta!("HTML", "html", ["html", "htm"], ["html"], false),
            Language::Css => lang_meta!("CSS", "css", ["css"], ["css"], false),
            Language::Scss => lang_meta!("SCSS", "scss", ["scss", "sass", "less"], ["scss", "sass"], false),
            Language::Vue => lang_meta!("Vue", "vue", ["vue"], ["vue"], false),
            Language::Svelte => lang_meta!("Svelte", "svelte", ["svelte"], ["svelte"], false),

            // Scripting Languages
            Language::Python => lang_meta!("Python", "python", ["py", "pyi", "pyw"], ["python", "py"], true),
            Language::Ruby => lang_meta!("Ruby", "ruby", ["rb", "rake", "gemspec"], ["ruby", "rb"], true),
            Language::Php => lang_meta!("PHP", "php", ["php", "phtml", "php3", "php4", "php5", "phps"], ["php"], false),
            Language::Perl => lang_meta!("Perl", "perl", ["pl", "pm"], ["perl", "pl"], false),
            Language::Lua => lang_meta!("Lua", "lua", ["lua"], ["lua"], false),
            Language::R => lang_meta!("R", "r", ["r"], ["r"], false),

            // Shell
            Language::Bash => lang_meta!("Bash", "bash", ["sh", "bash", "zsh", "fish"], ["bash", "sh", "shell"], true),
            Language::PowerShell => lang_meta!("PowerShell", "powershell", ["ps1", "psm1", "psd1"], ["powershell", "ps1"], false),

            // Mobile
            Language::Swift => lang_meta!("Swift", "swift", ["swift"], ["swift"], false),
            Language::ObjectiveC => lang_meta!("Objective-C", "objectivec", ["m", "mm"], ["objectivec", "objc"], false),
            Language::Dart => lang_meta!("Dart", "dart", ["dart"], ["dart"], false),

            // .NET
            Language::CSharp => lang_meta!("C#", "csharp", ["cs"], ["csharp", "c#", "cs"], false),
            Language::FSharp => lang_meta!("F#", "fsharp", ["fs", "fsx", "fsi"], ["fsharp", "f#", "fs"], false),
            Language::Vb => lang_meta!("VB.NET", "vb", ["vb"], ["vb", "vb.net"], false),

            // Functional
            Language::Elixir => lang_meta!("Elixir", "elixir", ["ex", "exs"], ["elixir", "ex"], false),
            Language::Erlang => lang_meta!("Erlang", "erlang", ["erl", "hrl"], ["erlang", "erl"], false),
            Language::Haskell => lang_meta!("Haskell", "haskell", ["hs", "lhs"], ["haskell", "hs"], false),
            Language::OCaml => lang_meta!("OCaml", "ocaml", ["ml", "mli"], ["ocaml", "ml"], false),
            Language::Crystal => lang_meta!("Crystal", "crystal", ["cr"], ["crystal", "cr"], false),
            Language::Julia => lang_meta!("Julia", "julia", ["jl"], ["julia", "jl"], false),

            // Data/Config
            Language::Sql => lang_meta!("SQL", "sql", ["sql"], ["sql"], false),
            Language::Yaml => lang_meta!("YAML", "yaml", ["yaml", "yml"], ["yaml", "yml"], false),
            Language::Json => lang_meta!("JSON", "json", ["json", "jsonc"], ["json"], false),
            Language::Toml => lang_meta!("TOML", "toml", ["toml"], ["toml"], false),
            Language::Xml => lang_meta!("XML", "xml", ["xml", "xsd", "xsl", "xslt"], ["xml"], false),
            Language::Markdown => lang_meta!("Markdown", "markdown", ["md", "markdown"], ["markdown", "md"], false),
            Language::Ini => lang_meta!("INI", "ini", ["ini", "cfg"], ["ini", "cfg"], false),

            // Other
            Language::Makefile => lang_meta!("Makefile", "makefile", [], ["makefile", "make"], false),
            Language::Dockerfile => lang_meta!("Dockerfile", "dockerfile", [], ["dockerfile", "docker"], false),
            Language::Proto => lang_meta!("Protocol Buffers", "protobuf", ["proto"], ["proto", "protobuf"], false),
            Language::GraphQL => lang_meta!("GraphQL", "graphql", ["graphql", "gql"], ["graphql", "gql"], false),
            Language::Wasm => lang_meta!("WebAssembly", "wasm", ["wat", "wast"], ["wasm", "webassembly"], false),

            Language::Unknown => lang_meta!("Unknown", "text", [], ["unknown", "text", ""], false),
        }
    }
}

// =============================================================================
// Language Enum Definition
// =============================================================================

/// Supported programming languages for code analysis.
///
/// Comprehensive language support for:
/// - Code parsing (tree-sitter backed)
/// - Syntax highlighting
/// - Documentation generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Language {
    // Systems Languages
    Rust,
    Go,
    C,
    Cpp,
    Zig,
    Nim,

    // JVM Languages
    Java,
    Kotlin,
    Scala,
    Groovy,
    Clojure,

    // Web Languages
    TypeScript,
    JavaScript,
    Tsx,
    Jsx,
    Html,
    Css,
    Scss,
    Vue,
    Svelte,

    // Scripting Languages
    Python,
    Ruby,
    Php,
    Perl,
    Lua,
    R,

    // Shell
    Bash,
    PowerShell,

    // Mobile
    Swift,
    ObjectiveC,
    Dart,

    // .NET
    CSharp,
    FSharp,
    Vb,

    // Functional
    Elixir,
    Erlang,
    Haskell,
    OCaml,
    Crystal,
    Julia,

    // Data/Config
    Sql,
    Yaml,
    Json,
    Toml,
    Xml,
    Markdown,
    Ini,

    // Other
    Makefile,
    Dockerfile,
    Proto,
    GraphQL,
    Wasm,

    #[default]
    Unknown,
}

// =============================================================================
// Language Methods (using metadata table)
// =============================================================================

impl Language {
    /// Display name (human-readable)
    pub fn as_str(&self) -> &'static str {
        self.meta().display_name
    }

    /// Syntax highlighting identifier (lowercase, for markdown code blocks)
    ///
    /// Returns the string to use in ```lang code blocks
    pub fn highlight_str(&self) -> &'static str {
        self.meta().highlight_str
    }

    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Self {
        let ext_lower = ext.to_lowercase();

        // Iterate through all variants to find matching extension
        for lang in Self::all_variants() {
            let meta = lang.meta();
            if meta.extensions.iter().any(|e| *e == ext_lower) {
                return *lang;
            }
        }

        Language::Unknown
    }

    /// Detect language from file path
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();

        // Check filename for special cases first
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            let lower = filename.to_lowercase();
            if lower == "makefile" || lower == "gnumakefile" {
                return Language::Makefile;
            }
            if lower == "dockerfile" || lower.starts_with("dockerfile.") {
                return Language::Dockerfile;
            }
        }

        // Then check extension
        path.extension()
            .and_then(|e| e.to_str())
            .map(Self::from_extension)
            .unwrap_or(Language::Unknown)
    }

    /// Check if this is a known language (not Unknown)
    pub fn is_known(&self) -> bool {
        !matches!(self, Language::Unknown)
    }

    /// Check if this language has tree-sitter parser support
    pub fn has_parser_support(&self) -> bool {
        self.meta().has_parser
    }

    /// Get all language variants for iteration
    fn all_variants() -> &'static [Language] {
        &[
            Language::Rust, Language::Go, Language::C, Language::Cpp,
            Language::Zig, Language::Nim, Language::Java, Language::Kotlin,
            Language::Scala, Language::Groovy, Language::Clojure,
            Language::TypeScript, Language::JavaScript, Language::Tsx,
            Language::Jsx, Language::Html, Language::Css, Language::Scss,
            Language::Vue, Language::Svelte, Language::Python, Language::Ruby,
            Language::Php, Language::Perl, Language::Lua, Language::R,
            Language::Bash, Language::PowerShell, Language::Swift,
            Language::ObjectiveC, Language::Dart, Language::CSharp,
            Language::FSharp, Language::Vb, Language::Elixir, Language::Erlang,
            Language::Haskell, Language::OCaml, Language::Crystal, Language::Julia,
            Language::Sql, Language::Yaml, Language::Json, Language::Toml,
            Language::Xml, Language::Markdown, Language::Ini, Language::Makefile,
            Language::Dockerfile, Language::Proto, Language::GraphQL, Language::Wasm,
        ]
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Language {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_lower = s.to_lowercase();

        for lang in Self::all_variants() {
            let meta = lang.meta();
            if meta.aliases.iter().any(|a| *a == s_lower) {
                return Ok(*lang);
            }
        }

        // Check Unknown explicitly
        if s_lower.is_empty() || s_lower == "unknown" || s_lower == "text" {
            return Ok(Language::Unknown);
        }

        Err(())
    }
}

// =============================================================================
// Standalone Functions (for direct syntax highlighting usage)
// =============================================================================

/// Detect language from file path and return syntax highlighting identifier.
///
/// Returns `Some("rust")`, `Some("python")`, etc. for known languages.
/// Returns `None` for unknown extensions.
///
/// This is the **canonical** function for detecting language from file paths.
/// Do NOT create duplicate implementations elsewhere.
///
/// # Examples
///
/// ```rust,ignore
/// use weavewiki::analyzer::parser::language::detect_language;
///
/// assert_eq!(detect_language("main.rs"), Some("rust"));
/// assert_eq!(detect_language("app.py"), Some("python"));
/// assert_eq!(detect_language("unknown.xyz"), None);
/// ```
pub fn detect_language<P: AsRef<Path>>(path: P) -> Option<&'static str> {
    let lang = Language::from_path(path);
    if lang.is_known() {
        Some(lang.highlight_str())
    } else {
        None
    }
}

/// Detect language with fallback to "text" for unknown extensions.
///
/// Use this when you need a non-optional string for syntax highlighting.
pub fn detect_language_or_text<P: AsRef<Path>>(path: P) -> &'static str {
    detect_language(path).unwrap_or("text")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("RS"), Language::Rust);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("tsx"), Language::Tsx);
        assert_eq!(Language::from_extension("jsx"), Language::Jsx);
        assert_eq!(Language::from_extension("kt"), Language::Kotlin);
        assert_eq!(Language::from_extension("unknown"), Language::Unknown);
    }

    #[test]
    fn test_from_path() {
        assert_eq!(Language::from_path("src/main.rs"), Language::Rust);
        assert_eq!(Language::from_path("test.py"), Language::Python);
        assert_eq!(Language::from_path("no_extension"), Language::Unknown);
        assert_eq!(Language::from_path("Component.tsx"), Language::Tsx);
    }

    #[test]
    fn test_special_filenames() {
        assert_eq!(Language::from_path("Makefile"), Language::Makefile);
        assert_eq!(Language::from_path("makefile"), Language::Makefile);
        assert_eq!(Language::from_path("Dockerfile"), Language::Dockerfile);
        assert_eq!(Language::from_path("Dockerfile.prod"), Language::Dockerfile);
    }

    #[test]
    fn test_highlight_str() {
        assert_eq!(Language::Rust.highlight_str(), "rust");
        assert_eq!(Language::Python.highlight_str(), "python");
        assert_eq!(Language::TypeScript.highlight_str(), "typescript");
        assert_eq!(Language::Tsx.highlight_str(), "tsx");
        assert_eq!(Language::Cpp.highlight_str(), "cpp");
        assert_eq!(Language::Unknown.highlight_str(), "text");
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("main.rs"), Some("rust"));
        assert_eq!(detect_language("app.py"), Some("python"));
        assert_eq!(detect_language("index.ts"), Some("typescript"));
        assert_eq!(detect_language("Component.tsx"), Some("tsx"));
        assert_eq!(detect_language("main.go"), Some("go"));
        assert_eq!(detect_language("unknown.xyz"), None);
    }

    #[test]
    fn test_detect_language_or_text() {
        assert_eq!(detect_language_or_text("main.rs"), "rust");
        assert_eq!(detect_language_or_text("unknown.xyz"), "text");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Language::Rust), "Rust");
        assert_eq!(format!("{}", Language::Cpp), "C++");
        assert_eq!(format!("{}", Language::CSharp), "C#");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("rust".parse::<Language>(), Ok(Language::Rust));
        assert_eq!("RUST".parse::<Language>(), Ok(Language::Rust));
        assert_eq!("c++".parse::<Language>(), Ok(Language::Cpp));
        assert_eq!("tsx".parse::<Language>(), Ok(Language::Tsx));
        assert_eq!("invalid_lang_xyz".parse::<Language>(), Err(()));
    }

    #[test]
    fn test_has_parser_support() {
        assert!(Language::Rust.has_parser_support());
        assert!(Language::Python.has_parser_support());
        assert!(Language::TypeScript.has_parser_support());
        assert!(!Language::Elixir.has_parser_support());
        assert!(!Language::Unknown.has_parser_support());
    }

    #[test]
    fn test_comprehensive_extensions() {
        // JVM
        assert_eq!(Language::from_extension("kt"), Language::Kotlin);
        assert_eq!(Language::from_extension("kts"), Language::Kotlin);
        assert_eq!(Language::from_extension("scala"), Language::Scala);
        assert_eq!(Language::from_extension("groovy"), Language::Groovy);
        assert_eq!(Language::from_extension("clj"), Language::Clojure);

        // Web
        assert_eq!(Language::from_extension("vue"), Language::Vue);
        assert_eq!(Language::from_extension("svelte"), Language::Svelte);
        assert_eq!(Language::from_extension("scss"), Language::Scss);

        // Functional
        assert_eq!(Language::from_extension("ex"), Language::Elixir);
        assert_eq!(Language::from_extension("hs"), Language::Haskell);
        assert_eq!(Language::from_extension("ml"), Language::OCaml);
        assert_eq!(Language::from_extension("cr"), Language::Crystal);
        assert_eq!(Language::from_extension("jl"), Language::Julia);

        // Data/Config
        assert_eq!(Language::from_extension("yaml"), Language::Yaml);
        assert_eq!(Language::from_extension("yml"), Language::Yaml);
        assert_eq!(Language::from_extension("json"), Language::Json);
        assert_eq!(Language::from_extension("toml"), Language::Toml);
        assert_eq!(Language::from_extension("md"), Language::Markdown);
    }

    #[test]
    fn test_serialization() {
        let lang = Language::Rust;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"Rust\"");

        let parsed: Language = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Language::Rust);
    }

    #[test]
    fn test_metadata_consistency() {
        // Ensure all variants have valid metadata
        for lang in Language::all_variants() {
            let meta = lang.meta();
            assert!(!meta.display_name.is_empty(), "Empty display name for {:?}", lang);
            assert!(!meta.highlight_str.is_empty(), "Empty highlight_str for {:?}", lang);
        }
    }
}
