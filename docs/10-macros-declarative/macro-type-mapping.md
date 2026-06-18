# 10.2 Rust 类型到 JSON Schema 映射

`#[tool]` 宏在编译期自动将 Rust 函数参数类型转换为 OpenAI/JSON Schema 兼容的参数定义。本章提供完整的类型映射参考和属性使用指南。

## 类型映射表

### 基本类型映射

| Rust 类型 | JSON Schema type | 说明 |
|-----------|-----------------|------|
| `String` | `"string"` | UTF-8 字符串 |
| `&str` | `"string"` | 字符串切片（参数场景少见） |
| `i8` | `"integer"` | 8 位有符号整数 |
| `i16` | `"integer"` | 16 位有符号整数 |
| `i32` | `"integer"` | 32 位有符号整数 |
| `i64` | `"integer"` | 64 位有符号整数（推荐） |
| `i128` | `"integer"` | 128 位有符号整数 |
| `isize` | `"integer"` | 平台相关有符号整数 |
| `u8` | `"integer"` | 8 位无符号整数 |
| `u16` | `"integer"` | 16 位无符号整数 |
| `u32` | `"integer"` | 32 位无符号整数 |
| `u64` | `"integer"` | 64 位无符号整数 |
| `u128` | `"integer"` | 128 位无符号整数 |
| `usize` | `"integer"` | 平台相关无符号整数 |
| `f32` | `"number"` | 32 位浮点数 |
| `f64` | `"number"` | 64 位浮点数（推荐） |
| `bool` | `"boolean"` | 布尔值 |

### 泛型容器类型映射

| Rust 类型 | JSON Schema | 说明 |
|-----------|------------|------|
| `Option<T>` | 基本类型 schema（不进入 required） | 可选参数，`is_option_type` 返回 `true` |
| `Vec<T>` | `{"type": "array", "items": <T>}` | 动态数组 |
| `HashMap<String, T>` | `{"type": "string"}` (回退) | 目前回退为字符串类型 |

### 解析逻辑

生成的代码在编译期执行以下匹配：

```rust
match type_str {
    "String" | "&str" | "str" => json!({"type": "string"}),
    "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
    "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => json!({"type": "integer"}),
    "f32" | "f64" => json!({"type": "number"}),
    "bool" => json!({"type": "boolean"}),
    _ => json!({"type": "string"}), // 未知类型回退
}
```

## `#[param]` 属性

`#[param]` 属性用于为函数参数添加元数据，最主要的用途是设置参数描述（`desc`）。

### 语法

```rust
#[param(desc = "参数的描述文本")]
#[param(description = "参数的描述文本")]  // desc 的完整形式别名
```

`desc` 和 `description` 是等价的，`desc` 只是更简洁的简写。

### 示例

```rust
#[tool(description = "在数据库中搜索记录")]
async fn db_search(
    #[param(desc = "搜索关键词")] query: String,
    #[param(desc = "每页结果数量，默认 20")] limit: Option<i64>,
    #[param(desc = "结果偏移量")] offset: Option<i64>,
    #[param(desc = "是否包含已删除记录")] include_deleted: Option<bool>,
) -> rust_agent_core::ToolResult {
    // ...
}
```

生成的 JSON Schema：

```json
{
    "type": "object",
    "properties": {
        "query": {
            "type": "string",
            "description": "搜索关键词"
        },
        "limit": {
            "type": "integer",
            "description": "每页结果数量，默认 20"
        },
        "offset": {
            "type": "integer",
            "description": "结果偏移量"
        },
        "include_deleted": {
            "type": "boolean",
            "description": "是否包含已删除记录"
        }
    },
    "required": ["query"]
}
```

只有 `query` 出现在 `required` 中，因为其余参数都是 `Option<T>` 类型。

## `is_option_type` 检测机制

`is_option_type` 函数通过 `syn` 的类型路径分析来判断参数是否为 `Option<T>`：

```rust
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        type_path.path.segments.last()
            .map(|s| s.ident == "Option")
            .unwrap_or(false)
    } else {
        false
    }
}
```

**关键行为**：

- `Option<i64>` → `true`（参数标记为可选，不进入 required）
- `Option<String>` → `true`（同上）
- `Vec<String>` → `false`（Vec 不走 Option 检测逻辑）
- `i64` → `false`（必需参数，进入 required）

检测为 `true` 的参数：
- 不会出现在 `required` 数组中
- 生成的 Schema 类型按内部 `T` 的实际类型映射（不是 `Option`）
- 反序列化时 `None` 值正常工作

## `Option<T>` 的类型处理

当参数类型为 `Option<T>` 时，Schema 生成使用 `T` 的原始类型而非 `Option`：

```rust
// Option<T> 的处理
if type_str.starts_with("Option<") {
    let inner = extract_inner_type(&type_str);
    return rust_type_str_to_tokens(&inner);
}
```

## `Vec<T>` 的数组映射

`Vec<T>` 生成标准的 JSON Schema 数组定义：

```rust
// Vec<T> 的处理
if type_str.starts_with("Vec<") {
    let inner = extract_inner_type(&type_str);
    let inner_schema = rust_type_str_to_tokens(&inner);
    return quote! {
        serde_json::json!({
            "type": "array",
            "items": #inner_schema
        })
    };
}
```

示例映射：

| Rust 类型 | 生成的 JSON Schema |
|-----------|-------------------|
| `Vec<String>` | `{"type": "array", "items": {"type": "string"}}` |
| `Vec<i64>` | `{"type": "array", "items": {"type": "integer"}}` |
| `Vec<f64>` | `{"type": "array", "items": {"type": "number"}}` |
| `Vec<bool>` | `{"type": "array", "items": {"type": "boolean"}}` |

## `extract_inner_type` — 泛型参数提取

从 `Type<T>` 形式的字符串中提取内部类型 `T`：

```rust
fn extract_inner_type(type_str: &str) -> String {
    let start = type_str.find('<').map(|i| i + 1).unwrap_or(0);
    let end = type_str.rfind('>').unwrap_or(type_str.len());
    type_str[start..end].to_string()
}
```

该函数仅处理单层泛型嵌套（如 `Option<String>`、`Vec<i64>`），不支持 `HashMap<String, Vec<i64>>` 等深层嵌套。

## 完整示例：复杂工具参数

```rust
#[tool(description = "执行高级搜索操作")]
async fn advanced_search(
    #[param(desc = "搜索查询字符串")] query: String,
    #[param(desc = "搜索分类过滤器")] categories: Vec<String>,
    #[param(desc = "最低评分阈值")] min_rating: Option<f64>,
    #[param(desc = "最大结果数")] max_results: Option<i64>,
    #[param(desc = "是否仅搜索已验证内容")] verified_only: Option<bool>,
    #[param(desc = "排序字段")] sort_by: Option<String>,
) -> rust_agent_core::ToolResult {
    // ...
}
```

生成的 JSON Schema：

```json
{
    "type": "object",
    "properties": {
        "query": {
            "type": "string",
            "description": "搜索查询字符串"
        },
        "categories": {
            "type": "array",
            "items": {"type": "string"},
            "description": "搜索分类过滤器"
        },
        "min_rating": {
            "type": "number",
            "description": "最低评分阈值"
        },
        "max_results": {
            "type": "integer",
            "description": "最大结果数"
        },
        "verified_only": {
            "type": "boolean",
            "description": "是否仅搜索已验证内容"
        },
        "sort_by": {
            "type": "string",
            "description": "排序字段"
        }
    },
    "required": ["query", "categories"]
}
```

## 注意事项

1. **不支持深层嵌套泛型**：如 `HashMap<String, Vec<Option<i64>>>` 会因 `extract_inner_type` 的简单实现而产生不正确的结果
2. **自定义类型回退为 string**：不在内置映射表中的 Rust 类型（如自定义结构体、枚举）会回退到 `{"type": "string"}`
3. **`#[param]` 属性位置**：必须直接放在参数声明之前，不支持其他位置
4. **`#[tool]` 仅支持 async fn 和 struct**：不支持普通 fn、trait 方法或其他项目类型
