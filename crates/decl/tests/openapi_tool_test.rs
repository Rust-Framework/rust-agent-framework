#![cfg(all(feature = "yaml", feature = "openapi"))]

//! OpenAPI 工具声明式解析测试

use rust_agent_decl::resolver::ToolResolver;
use rust_agent_decl::tools::ToolDecl;

#[tokio::test]
async fn resolve_openapi_tool_from_file_spec() {
    let spec_path = std::env::temp_dir().join("raf-openapi-test.json");
    std::fs::write(
        &spec_path,
        r#"{
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com"}],
            "paths": {
                "/health": {
                    "get": {
                        "operationId": "healthCheck",
                        "summary": "Health check"
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let spec_url = format!("file://{}", spec_path.display());
    let decl = ToolDecl::OpenApi {
        name: "healthCheck".into(),
        spec_url,
        operation_id: Some("healthCheck".into()),
    };

    let resolver = ToolResolver::new();
    let tool = resolver.resolve(&decl).await.expect("openapi tool resolves");
    assert_eq!(tool.name(), "healthCheck");
    assert_eq!(tool.kind(), "openapi");
}
