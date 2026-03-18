//! Service Boundary Detection
//!
//! Detects service boundaries in the project for generating service-specific rules.
//! Works with both monorepos (multiple services) and single-service projects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pipeline::context::{FileRegistryExt, VerifiedFileRegistry};
use crate::types::module_map::DetectedModule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedService {
    pub service_id: String,
    pub name: String,
    pub path: String,
    pub service_type: ServiceType,
    pub modules: Vec<String>,
    pub interfaces: Vec<ServiceInterface>,
    pub dependencies: Vec<ServiceDependency>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceType {
    Api,
    Worker,
    Gateway,
    Library,
    Cli,
    Web,
}

impl std::fmt::Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api => write!(f, "API"),
            Self::Worker => write!(f, "Worker"),
            Self::Gateway => write!(f, "Gateway"),
            Self::Library => write!(f, "Library"),
            Self::Cli => write!(f, "CLI"),
            Self::Web => write!(f, "Web"),
        }
    }
}

impl ServiceType {
    pub fn from_indicators(indicators: &ServiceIndicators) -> Self {
        if indicators.has_http_server {
            if indicators.has_grpc || indicators.has_graphql {
                ServiceType::Gateway
            } else {
                ServiceType::Api
            }
        } else if indicators.has_worker || indicators.has_queue_consumer {
            ServiceType::Worker
        } else if indicators.has_cli_entry {
            ServiceType::Cli
        } else if indicators.has_web_framework {
            ServiceType::Web
        } else {
            ServiceType::Library
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServiceIndicators {
    pub has_http_server: bool,
    pub has_grpc: bool,
    pub has_graphql: bool,
    pub has_worker: bool,
    pub has_queue_consumer: bool,
    pub has_cli_entry: bool,
    pub has_web_framework: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInterface {
    pub interface_type: InterfaceType,
    pub endpoints: Vec<String>,
    pub protocol: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceType {
    Http,
    Grpc,
    GraphQL,
    WebSocket,
    Queue,
    Internal,
}

impl std::fmt::Display for InterfaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "HTTP"),
            Self::Grpc => write!(f, "gRPC"),
            Self::GraphQL => write!(f, "GraphQL"),
            Self::WebSocket => write!(f, "WebSocket"),
            Self::Queue => write!(f, "Queue"),
            Self::Internal => write!(f, "Internal"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDependency {
    pub target_service: String,
    pub dependency_type: DependencyType,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyType {
    Http,
    Grpc,
    Queue,
    Database,
    Cache,
    Internal,
}

impl std::fmt::Display for DependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "HTTP"),
            Self::Grpc => write!(f, "gRPC"),
            Self::Queue => write!(f, "Queue"),
            Self::Database => write!(f, "Database"),
            Self::Cache => write!(f, "Cache"),
            Self::Internal => write!(f, "Internal"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceDetectionResult {
    pub services: Vec<DetectedService>,
    pub is_microservices: bool,
    pub shared_libraries: Vec<String>,
    pub service_graph: ServiceGraph,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceGraph {
    pub nodes: Vec<ServiceNode>,
    pub edges: Vec<ServiceEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceNode {
    pub service_id: String,
    pub service_type: ServiceType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEdge {
    pub from_service: String,
    pub to_service: String,
    pub dependency_type: DependencyType,
}

/// Minimum confidence to auto-classify as service (Phase 1 - high confidence)
const HIGH_CONFIDENCE_THRESHOLD: f64 = 0.8;

/// Minimum confidence to include as candidate (Phase 2 - LLM would verify)
const LOW_CONFIDENCE_THRESHOLD: f64 = 0.3;

const SHARED_LIBRARY_PATTERNS: &[&str] = &["common", "shared", "lib", "pkg", "internal", "utils"];

pub struct ServiceDetector {
    project_root: PathBuf,
}

/// Confidence-scored service candidate
#[derive(Debug, Clone)]
pub struct ServiceCandidate {
    pub module_id: String,
    pub confidence: f64,
    pub signals: Vec<String>,
}

impl ServiceDetector {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    pub fn detect_from_modules(
        &self,
        modules: &[DetectedModule],
        registry: &VerifiedFileRegistry,
    ) -> ServiceDetectionResult {
        let mut services = Vec::new();
        let mut module_to_service: HashMap<String, String> = HashMap::new();

        for module in modules {
            let candidate = self.score_service_candidate(module, registry);
            // Phase 1: Only accept high-confidence candidates programmatically
            // Low-confidence candidates would go to Phase 2 (LLM) when integrated
            if candidate.confidence >= HIGH_CONFIDENCE_THRESHOLD {
                let indicators = self.detect_indicators(module, registry);
                let service_type = ServiceType::from_indicators(&indicators);
                let primary_path = module.paths.first().cloned().unwrap_or_default();

                let service = DetectedService {
                    service_id: module.module_id.clone(),
                    name: humanize_name(&module.module_id),
                    path: primary_path.clone(),
                    service_type,
                    modules: vec![module.module_id.clone()],
                    interfaces: self.detect_interfaces(&indicators, &primary_path, registry),
                    dependencies: vec![],
                };

                module_to_service.insert(module.module_id.clone(), service.service_id.clone());
                services.push(service);
            } else if candidate.confidence >= LOW_CONFIDENCE_THRESHOLD {
                tracing::debug!(
                    module = %module.module_id,
                    confidence = candidate.confidence,
                    signals = ?candidate.signals,
                    "Uncertain service candidate (would need LLM verification)"
                );
            }
        }

        self.detect_service_dependencies(&mut services, registry);

        let shared_libraries = detect_shared_libraries(modules, &module_to_service);
        let service_graph = build_service_graph(&services);

        ServiceDetectionResult {
            is_microservices: services.len() > 1,
            services,
            shared_libraries,
            service_graph,
        }
    }

    /// Score a module's likelihood of being an independent service.
    ///
    /// Returns a confidence score (0.0-1.0) based on clear, unambiguous signals.
    /// Follows the two-phase pattern: only high-confidence signals are used
    /// programmatically; uncertain cases should defer to LLM.
    fn score_service_candidate(
        &self,
        module: &DetectedModule,
        registry: &VerifiedFileRegistry,
    ) -> ServiceCandidate {
        let primary_path = module
            .paths
            .first()
            .map(|s| s.trim_end_matches('/'))
            .unwrap_or("");
        if primary_path.is_empty() {
            return ServiceCandidate {
                module_id: module.module_id.clone(),
                confidence: 0.0,
                signals: vec![],
            };
        }

        let mut confidence: f64 = 0.0;
        let mut signals = Vec::new();

        // Strong signal: Dockerfile (universally means "deployable unit")
        let has_dockerfile = registry.file_exists(&format!("{}/Dockerfile", primary_path));
        if has_dockerfile {
            confidence += 0.4;
            signals.push("Dockerfile".into());
        }

        // Strong signal: main entry point
        let has_main = !registry
            .files_matching(&format!("{}/main", primary_path))
            .is_empty()
            || !registry
                .files_matching(&format!("{}/cmd", primary_path))
                .is_empty()
            || !registry
                .files_matching(&format!("{}/src/main", primary_path))
                .is_empty();
        if has_main {
            confidence += 0.4;
            signals.push("main entry point".into());
        }

        // Moderate signal: own build manifest
        let has_manifest =
            registry.file_exists(&format!("{}/package.json", primary_path))
                || registry.file_exists(&format!("{}/Cargo.toml", primary_path))
                || registry.file_exists(&format!("{}/go.mod", primary_path))
                || registry.file_exists(&format!("{}/build.gradle", primary_path));
        if has_manifest {
            confidence += 0.2;
            signals.push("own manifest".into());
        }

        // Weak signal: docker-compose reference
        if registry.file_exists(&format!("{}/docker-compose.yml", primary_path))
            || registry.file_exists(&format!("{}/docker-compose.yaml", primary_path))
        {
            confidence += 0.1;
            signals.push("docker-compose".into());
        }

        ServiceCandidate {
            module_id: module.module_id.clone(),
            confidence: confidence.min(1.0),
            signals,
        }
    }

    fn detect_indicators(
        &self,
        module: &DetectedModule,
        registry: &VerifiedFileRegistry,
    ) -> ServiceIndicators {
        let primary_path = module.paths.first().map(|s| s.as_str()).unwrap_or("");
        let files: Vec<&str> = registry
            .all_files()
            .map(String::as_str)
            .filter(|f| f.starts_with(primary_path))
            .collect();

        let file_path_hints: Vec<_> = files.iter().map(|f| f.to_lowercase()).collect();

        ServiceIndicators {
            has_http_server: file_path_hints
                .iter()
                .any(|f| f.contains("server") || f.contains("handler") || f.contains("router")),
            has_grpc: file_path_hints
                .iter()
                .any(|f| f.contains("grpc") || f.contains(".proto")),
            has_graphql: file_path_hints.iter().any(|f| f.contains("graphql")),
            has_worker: file_path_hints
                .iter()
                .any(|f| f.contains("worker") || f.contains("job")),
            has_queue_consumer: file_path_hints
                .iter()
                .any(|f| f.contains("consumer") || f.contains("subscriber")),
            has_cli_entry: file_path_hints
                .iter()
                .any(|f| f.contains("/cli") || f.contains("/cmd")),
            has_web_framework: file_path_hints
                .iter()
                .any(|f| f.contains("component") || f.contains("pages") || f.contains("views")),
        }
    }

    fn detect_interfaces(
        &self,
        indicators: &ServiceIndicators,
        service_path: &str,
        registry: &VerifiedFileRegistry,
    ) -> Vec<ServiceInterface> {
        let mut interfaces = Vec::new();

        if indicators.has_http_server {
            let endpoints = self.extract_http_endpoints(service_path, registry);
            interfaces.push(ServiceInterface {
                interface_type: InterfaceType::Http,
                endpoints,
                protocol: "HTTP/1.1".to_string(),
            });
        }

        if indicators.has_grpc {
            let endpoints = self.extract_grpc_services(service_path, registry);
            interfaces.push(ServiceInterface {
                interface_type: InterfaceType::Grpc,
                endpoints,
                protocol: "gRPC".to_string(),
            });
        }

        if indicators.has_graphql {
            interfaces.push(ServiceInterface {
                interface_type: InterfaceType::GraphQL,
                endpoints: vec!["GraphQL endpoint".to_string()],
                protocol: "GraphQL".to_string(),
            });
        }

        if indicators.has_queue_consumer {
            interfaces.push(ServiceInterface {
                interface_type: InterfaceType::Queue,
                endpoints: vec![],
                protocol: "AMQP".to_string(),
            });
        }

        interfaces
    }

    fn extract_http_endpoints(&self, service_path: &str, registry: &VerifiedFileRegistry) -> Vec<String> {
        let mut endpoints = Vec::new();
        let route_files: Vec<&str> = registry
            .all_files()
            .map(String::as_str)
            .filter(|f| {
                f.starts_with(service_path)
                    && (f.contains("route")
                        || f.contains("handler")
                        || f.contains("controller")
                        || f.contains("endpoint"))
            })
            .collect();

        for file in &route_files {
            let full_path = self.project_root.join(file);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                for endpoint in extract_route_patterns(&content) {
                    endpoints.push(endpoint);
                }
            }
        }

        endpoints
    }

    fn extract_grpc_services(&self, service_path: &str, registry: &VerifiedFileRegistry) -> Vec<String> {
        let mut services = Vec::new();
        let proto_files: Vec<&str> = registry
            .all_files()
            .map(String::as_str)
            .filter(|f| f.starts_with(service_path) && f.ends_with(".proto"))
            .collect();

        for file in &proto_files {
            let full_path = self.project_root.join(file);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                for service in extract_grpc_service_names(&content) {
                    services.push(service);
                }
            }
        }

        services
    }

    fn detect_service_dependencies(
        &self,
        services: &mut [DetectedService],
        registry: &VerifiedFileRegistry,
    ) {
        let service_names: Vec<String> = services.iter().map(|s| s.service_id.clone()).collect();

        for service in services.iter_mut() {
            let service_files: Vec<&str> = registry
                .all_files()
                .map(String::as_str)
                .filter(|f| f.starts_with(service.path.as_str()))
                .collect();

            for file in service_files {
                let full_path = self.project_root.join(file);
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let content_lower = content.to_lowercase();
                    for other_service in &service_names {
                        if other_service == &service.service_id {
                            continue;
                        }
                        let pattern = format!(r"\b{}\b", regex::escape(&other_service.to_lowercase()));
                        let matches = regex::Regex::new(&pattern)
                            .map(|re| re.is_match(&content_lower))
                            .unwrap_or(false);
                        if matches
                            && !service
                                .dependencies
                                .iter()
                                .any(|d| d.target_service == *other_service)
                        {
                            let dep_type = infer_dependency_type(&content, other_service);
                            service.dependencies.push(ServiceDependency {
                                target_service: other_service.clone(),
                                dependency_type: dep_type,
                                description: format!(
                                    "References {} via {:?}",
                                    other_service, dep_type
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn extract_route_patterns(content: &str) -> Vec<String> {
    use std::sync::LazyLock;

    static ROUTE_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        [
            r#"@(Get|Post|Put|Delete|Patch)\s*\(\s*["']([^"']+)["']\s*\)"#,
            r#"(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#,
            r#"router\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']"#,
            r#"path\s*=\s*["']([^"']+)["']"#,
        ]
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
    });

    let mut routes = Vec::new();
    for re in ROUTE_PATTERNS.iter() {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(cap.len() - 1) {
                routes.push(m.as_str().to_string());
            }
        }
    }
    routes
}

fn extract_grpc_service_names(content: &str) -> Vec<String> {
    use std::sync::LazyLock;

    static GRPC_SERVICE_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"service\s+(\w+)\s*\{").unwrap());

    let mut services = Vec::new();
    for cap in GRPC_SERVICE_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            services.push(m.as_str().to_string());
        }
    }
    services
}

fn infer_dependency_type(content: &str, _target: &str) -> DependencyType {
    let content_lower = content.to_lowercase();

    if content_lower.contains("http")
        || content_lower.contains("fetch")
        || content_lower.contains("axios")
    {
        DependencyType::Http
    } else if content_lower.contains("grpc") {
        DependencyType::Grpc
    } else if content_lower.contains("queue")
        || content_lower.contains("kafka")
        || content_lower.contains("rabbitmq")
    {
        DependencyType::Queue
    } else if content_lower.contains("redis") || content_lower.contains("cache") {
        DependencyType::Cache
    } else if content_lower.contains("database")
        || content_lower.contains("postgres")
        || content_lower.contains("mysql")
    {
        DependencyType::Database
    } else {
        DependencyType::Internal
    }
}

fn detect_shared_libraries(
    modules: &[DetectedModule],
    module_to_service: &HashMap<String, String>,
) -> Vec<String> {
    modules
        .iter()
        .filter(|m| {
            !module_to_service.contains_key(&m.module_id)
                && SHARED_LIBRARY_PATTERNS
                    .iter()
                    .any(|p| m.module_id.contains(p))
        })
        .map(|m| m.module_id.clone())
        .collect()
}

fn build_service_graph(services: &[DetectedService]) -> ServiceGraph {
    let nodes: Vec<ServiceNode> = services
        .iter()
        .map(|s| ServiceNode {
            service_id: s.service_id.clone(),
            service_type: s.service_type,
        })
        .collect();

    let edges: Vec<ServiceEdge> = services
        .iter()
        .flat_map(|s| {
            s.dependencies.iter().map(|d| ServiceEdge {
                from_service: s.service_id.clone(),
                to_service: d.target_service.clone(),
                dependency_type: d.dependency_type,
            })
        })
        .collect();

    ServiceGraph { nodes, edges }
}

fn humanize_name(name: &str) -> String {
    name.replace(['_', '-'], " ")
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type_from_indicators() {
        let api_indicators = ServiceIndicators {
            has_http_server: true,
            ..Default::default()
        };
        assert_eq!(
            ServiceType::from_indicators(&api_indicators),
            ServiceType::Api
        );

        let gateway_indicators = ServiceIndicators {
            has_http_server: true,
            has_grpc: true,
            ..Default::default()
        };
        assert_eq!(
            ServiceType::from_indicators(&gateway_indicators),
            ServiceType::Gateway
        );

        let worker_indicators = ServiceIndicators {
            has_worker: true,
            ..Default::default()
        };
        assert_eq!(
            ServiceType::from_indicators(&worker_indicators),
            ServiceType::Worker
        );
    }

    #[test]
    fn test_humanize_name() {
        assert_eq!(humanize_name("user_service"), "User Service");
        assert_eq!(humanize_name("api-gateway"), "Api Gateway");
    }

    #[test]
    fn test_extract_route_patterns() {
        let content = r#"
            @Get("/users")
            @Post("/users/:id")
            router.get("/api/v1/health")
        "#;
        let routes = extract_route_patterns(content);
        assert!(routes.contains(&"/users".to_string()));
    }

    #[test]
    fn test_extract_grpc_service_names() {
        let proto = r#"
            service UserService {
                rpc GetUser(GetUserRequest) returns (User);
            }
            service AuthService {
                rpc Login(LoginRequest) returns (Token);
            }
        "#;
        let services = extract_grpc_service_names(proto);
        assert!(services.contains(&"UserService".to_string()));
        assert!(services.contains(&"AuthService".to_string()));
    }

    #[test]
    fn test_infer_dependency_type() {
        assert_eq!(
            infer_dependency_type("axios.get(url)", "service"),
            DependencyType::Http
        );
        assert_eq!(
            infer_dependency_type("grpcClient.call()", "service"),
            DependencyType::Grpc
        );
        assert_eq!(
            infer_dependency_type("kafka.send()", "service"),
            DependencyType::Queue
        );
    }

    #[test]
    fn test_confidence_scoring_high() {
        let dir = tempfile::tempdir().unwrap();
        let detector = ServiceDetector::new(dir.path());
        let mut registry = VerifiedFileRegistry::empty();
        registry.register_test_file("services/api/Dockerfile");
        registry.register_test_file("services/api/src/main.rs");
        registry.register_test_file("services/api/Cargo.toml");
        let module = DetectedModule::new("api", "API service")
            .paths(vec!["services/api/".into()]);

        let candidate = detector.score_service_candidate(&module, &registry);
        assert!(
            candidate.confidence >= HIGH_CONFIDENCE_THRESHOLD,
            "Dockerfile + main + manifest should be high confidence: {}",
            candidate.confidence
        );
        assert!(candidate.signals.contains(&"Dockerfile".to_string()));
        assert!(candidate.signals.contains(&"main entry point".to_string()));
    }

    #[test]
    fn test_confidence_scoring_low() {
        let dir = tempfile::tempdir().unwrap();
        let detector = ServiceDetector::new(dir.path());
        let mut registry = VerifiedFileRegistry::empty();
        registry.register_test_file("libs/utils/src/lib.rs");
        let module = DetectedModule::new("utils", "Utility library")
            .paths(vec!["libs/utils/".into()]);

        let candidate = detector.score_service_candidate(&module, &registry);
        assert!(
            candidate.confidence < LOW_CONFIDENCE_THRESHOLD,
            "Library module should have low confidence: {}",
            candidate.confidence
        );
    }

    #[test]
    fn test_confidence_scoring_medium() {
        let dir = tempfile::tempdir().unwrap();
        let detector = ServiceDetector::new(dir.path());
        let mut registry = VerifiedFileRegistry::empty();
        registry.register_test_file("services/worker/package.json");
        registry.register_test_file("services/worker/docker-compose.yml");
        let module = DetectedModule::new("worker", "Worker service")
            .paths(vec!["services/worker/".into()]);

        let candidate = detector.score_service_candidate(&module, &registry);
        assert!(
            candidate.confidence >= LOW_CONFIDENCE_THRESHOLD
                && candidate.confidence < HIGH_CONFIDENCE_THRESHOLD,
            "Manifest + docker-compose should be medium confidence: {}",
            candidate.confidence
        );
    }
}
