use serde::{Deserialize, Serialize};

/// 容器/托管 Agent 数据（kind = "hosted"），与 MAF AgentSchema v1.0 对齐。
///
/// 表示运行在提供商（如 Azure Foundry）托管环境中的基于容器的 Agent。
/// 支持容器镜像部署、Dockerfile 构建、资源分配、环境变量和基于代码（ZIP）的部署。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerAgentData {
    /// 容器化 Agent 使用的协议。
    pub protocols: Vec<ProtocolVersionRecord>,

    /// 容器镜像路径（例如 `myregistry.azurecr.io/my-agent`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// 用于部署的 Dockerfile 路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dockerfile_path: Option<String>,

    /// 容器的资源分配。
    pub resources: ContainerResources,

    /// 在容器中设置的环境变量。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment_variables: Vec<EnvironmentVariable>,

    /// 基于代码（ZIP 上传）部署的配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_configuration: Option<CodeConfiguration>,
}

/// 容器 Agent 通信的协议版本记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolVersionRecord {
    /// 协议名称（例如 "responses"）。
    pub protocol: String,
    /// 可选的协议版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// 容器的资源分配规格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerResources {
    /// CPU 分配（例如 "1"、"0.5"）。
    pub cpu: String,
    /// 内存分配（例如 "2Gi"、"512Mi"）。
    pub memory: String,
}

/// 容器的环境变量。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    /// 变量名称。
    pub name: String,
    /// 变量值。
    pub value: String,
}

/// 基于代码（ZIP）部署的配置。
/// 存在时，Agent 源代码将直接上传，无需构建容器镜像。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeConfiguration {
    /// 代码的源路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// 运行时规范。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

impl ContainerAgentData {
    /// 使用协议和资源创建容器 Agent。
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

    /// 设置容器镜像。
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// 设置 Dockerfile 路径。
    pub fn with_dockerfile(mut self, path: impl Into<String>) -> Self {
        self.dockerfile_path = Some(path.into());
        self
    }

    /// 添加环境变量。
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment_variables.push(EnvironmentVariable {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// 设置代码配置。
    pub fn with_code_configuration(mut self, cfg: CodeConfiguration) -> Self {
        self.code_configuration = Some(cfg);
        self
    }
}

impl ProtocolVersionRecord {
    /// 使用协议名称创建协议记录。
    pub fn new(protocol: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            version: None,
        }
    }

    /// 使用名称和版本创建协议记录。
    pub fn with_version(protocol: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            version: Some(version.into()),
        }
    }
}

impl ContainerResources {
    /// 使用 CPU 和内存创建资源分配。
    pub fn new(cpu: impl Into<String>, memory: impl Into<String>) -> Self {
        Self {
            cpu: cpu.into(),
            memory: memory.into(),
        }
    }
}
