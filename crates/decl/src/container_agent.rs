use serde::{Deserialize, Serialize};

/// Container/hosted agent data (kind = "hosted").
/// Aligns with MAF AgentSchema v1.0 `ContainerAgent`.
///
/// Represents a container-based agent that runs in a hosted environment
/// managed by the provider (e.g., Azure Foundry). Supports container
/// image deployment, Dockerfile-based builds, resource allocation,
/// environment variables, and code-based (ZIP) deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerAgentData {
    /// Protocols used by the containerized agent.
    pub protocols: Vec<ProtocolVersionRecord>,

    /// Container image path (e.g., `myregistry.azurecr.io/my-agent`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Path to a Dockerfile for deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dockerfile_path: Option<String>,

    /// Resource allocation for the container.
    pub resources: ContainerResources,

    /// Environment variables to set in the container.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment_variables: Vec<EnvironmentVariable>,

    /// Configuration for code-based (ZIP upload) deployment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_configuration: Option<CodeConfiguration>,
}

/// Protocol version record for container agent communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolVersionRecord {
    /// Protocol name (e.g., "responses").
    pub protocol: String,
    /// Optional protocol version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Resource allocation specification for a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerResources {
    /// CPU allocation (e.g., "1", "0.5").
    pub cpu: String,
    /// Memory allocation (e.g., "2Gi", "512Mi").
    pub memory: String,
}

/// Environment variable for the container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    /// Variable name.
    pub name: String,
    /// Variable value.
    pub value: String,
}

/// Configuration for code-based (ZIP) deployment.
/// When present, agent source code is uploaded directly instead of building
/// a container image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeConfiguration {
    /// Source path for the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// Runtime specification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

impl ContainerAgentData {
    /// Create a container agent with protocol and resources.
    pub fn new(protocols: Vec<ProtocolVersionRecord>, resources: ContainerResources) -> Self {
        Self {
            protocols,
            image: None,
            dockerfile_path: None,
            resources,
            environment_variables: Vec::new(),
            code_configuration: None,
        }
    }

    /// Set the container image.
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// Set the Dockerfile path.
    pub fn with_dockerfile(mut self, path: impl Into<String>) -> Self {
        self.dockerfile_path = Some(path.into());
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment_variables.push(EnvironmentVariable {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// Set code configuration.
    pub fn with_code_configuration(mut self, cfg: CodeConfiguration) -> Self {
        self.code_configuration = Some(cfg);
        self
    }
}

impl ProtocolVersionRecord {
    /// Create a protocol record with name.
    pub fn new(protocol: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            version: None,
        }
    }

    /// Create a protocol record with name and version.
    pub fn with_version(protocol: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            version: Some(version.into()),
        }
    }
}

impl ContainerResources {
    /// Create resource allocation with CPU and memory.
    pub fn new(cpu: impl Into<String>, memory: impl Into<String>) -> Self {
        Self {
            cpu: cpu.into(),
            memory: memory.into(),
        }
    }
}
