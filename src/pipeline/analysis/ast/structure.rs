//! AST Structure Types
//!
//! Core types for representing extracted AST information.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AstProjectStructure {
    pub root: String,
    pub modules: Vec<ModuleInfo>,
    pub layers: Vec<ArchitecturalLayer>,
    pub entry_points: Vec<EntryPointInfo>,
    pub total_files: usize,
    pub total_functions: usize,
    pub total_types: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub files: Vec<String>,
    pub public_items: Vec<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalLayer {
    pub name: String,
    pub modules: Vec<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPointInfo {
    pub path: String,
    pub name: String,
    pub kind: EntryPointKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntryPointKind {
    MainFunction,
    LibraryRoot,
    BinaryEntry,
    TestEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub visibility: Visibility,
    pub is_async: bool,
    pub parameters: Vec<ParameterInfo>,
    pub return_type: Option<String>,
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    pub type_annotation: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Visibility {
    Public,
    #[default]
    Private,
    Crate,
    Super,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    pub kind: TypeKind,
    pub line_start: usize,
    pub line_end: usize,
    pub visibility: Visibility,
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<String>,
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    Interface,
    Class,
    TypeAlias,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub type_annotation: Option<String>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    pub path: String,
    pub items: Vec<String>,
    pub line: usize,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportInfo {
    pub name: String,
    pub kind: ExportKind,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Type,
    Constant,
    Module,
    ReExport,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComplexityMetrics {
    pub cyclomatic: usize,
    pub cognitive: usize,
    pub lines_of_code: usize,
    pub comment_lines: usize,
    pub nesting_depth: usize,
}

impl ComplexityMetrics {
    pub fn is_complex(&self) -> bool {
        self.cyclomatic > 10 || self.cognitive > 15 || self.nesting_depth > 4
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublicApiSurface {
    pub functions: Vec<ApiFunctionInfo>,
    pub types: Vec<ApiTypeInfo>,
    pub constants: Vec<ApiConstantInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFunctionInfo {
    pub name: String,
    pub path: String,
    pub signature: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTypeInfo {
    pub name: String,
    pub path: String,
    pub kind: TypeKind,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConstantInfo {
    pub name: String,
    pub path: String,
    pub type_annotation: Option<String>,
}
