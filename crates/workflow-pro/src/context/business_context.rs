use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 变量类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VariableType {
    String,
    Number,
    Boolean,
    Object,
    Array,
}

/// 变量 Schema 定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableSchema {
    pub name: String,
    pub var_type: VariableType,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
    pub description: Option<String>,
    pub validation: Option<serde_json::Value>, // JSON Schema 子集
}

impl VariableSchema {
    pub fn string(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            var_type: VariableType::String,
            required: false,
            default_value: None,
            description: None,
            validation: None,
        }
    }

    pub fn number(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            var_type: VariableType::Number,
            required: false,
            default_value: None,
            description: None,
            validation: None,
        }
    }

    pub fn boolean(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            var_type: VariableType::Boolean,
            required: false,
            default_value: None,
            description: None,
            validation: None,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn with_default(mut self, value: serde_json::Value) -> Self {
        self.default_value = Some(value);
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// 验证值是否符合 Schema。
    pub fn validate(&self, value: &serde_json::Value) -> Result<(), String> {
        match self.var_type {
            VariableType::String => {
                if !value.is_string() {
                    return Err(format!("变量 '{}' 应为 String 类型", self.name));
                }
            }
            VariableType::Number => {
                if !value.is_number() {
                    return Err(format!("变量 '{}' 应为 Number 类型", self.name));
                }
            }
            VariableType::Boolean => {
                if !value.is_boolean() {
                    return Err(format!("变量 '{}' 应为 Boolean 类型", self.name));
                }
            }
            VariableType::Object => {
                if !value.is_object() {
                    return Err(format!("变量 '{}' 应为 Object 类型", self.name));
                }
            }
            VariableType::Array => {
                if !value.is_array() {
                    return Err(format!("变量 '{}' 应为 Array 类型", self.name));
                }
            }
        }
        Ok(())
    }
}

/// 业务变量容器 —— 带 Schema 验证的类型化变量存储。
#[derive(Debug, Clone)]
pub struct BusinessVariables {
    schemas: HashMap<String, VariableSchema>,
    values: HashMap<String, serde_json::Value>,
}

impl BusinessVariables {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
            values: HashMap::new(),
        }
    }

    /// 注册变量 Schema。
    pub fn register(mut self, schema: VariableSchema) -> Self {
        if let Some(default) = &schema.default_value {
            self.values.insert(schema.name.clone(), default.clone());
        }
        self.schemas.insert(schema.name.clone(), schema);
        self
    }

    /// 设置变量（带类型校验）。
    pub fn set(&mut self, name: &str, value: serde_json::Value) -> Result<(), String> {
        if let Some(schema) = self.schemas.get(name) {
            schema.validate(&value)?;
        }
        self.values.insert(name.to_string(), value);
        Ok(())
    }

    /// 获取变量值。
    pub fn get(&self, name: &str) -> Option<&serde_json::Value> {
        self.values.get(name)
    }

    /// 获取所有变量。
    pub fn all(&self) -> &HashMap<String, serde_json::Value> {
        &self.values
    }

    /// 检查所有 required 变量是否已设置。
    pub fn validate_required(&self) -> Result<(), Vec<String>> {
        let mut missing = Vec::new();
        for schema in self.schemas.values() {
            if schema.required && !self.values.contains_key(&schema.name) {
                missing.push(schema.name.clone());
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// 转换为 JSON。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.values).unwrap_or(serde_json::Value::Null)
    }
}

impl Default for BusinessVariables {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_set() {
        let mut vars = BusinessVariables::new()
            .register(VariableSchema::string("name").required())
            .register(VariableSchema::number("count").with_default(serde_json::json!(0)));

        assert!(vars.set("name", serde_json::json!("test")).is_ok());
        assert!(vars.set("count", serde_json::json!(42)).is_ok());
        assert_eq!(vars.get("name").unwrap().as_str(), Some("test"));
    }

    #[test]
    fn test_type_validation() {
        let mut vars = BusinessVariables::new()
            .register(VariableSchema::number("count"));

        assert!(vars.set("count", serde_json::json!("not_a_number")).is_err());
        assert!(vars.set("count", serde_json::json!(42)).is_ok());
    }

    #[test]
    fn test_required_validation() {
        let vars = BusinessVariables::new()
            .register(VariableSchema::string("name").required());

        let result = vars.validate_required();
        assert!(result.is_err());
    }
}
