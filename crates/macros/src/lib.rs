use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, ExprLit, Lit};
use syn::parse::Parser;

/// `#[tool]` 属性宏——简化 ITool 定义。
///
/// # 用于异步函数
///
/// ```ignore
/// #[tool(description = "Echoes back the input text", kind = "function")]
/// async fn echo(#[param(desc = "Text to echo")] text: String) -> rust_agent_core::ToolResult {
///     rust_agent_core::ToolResult::success(serde_json::json!({"echo": text}))
/// }
/// ```
///
/// 生成一个实现 `ITool` 的结构体（函数名的帕斯卡命名），
/// 自动派生 JSON schema 和参数反序列化。kind() 返回 `"function"`。
///
/// # 用于 impl 块（推荐：持有状态的工具）
///
/// ```ignore
/// pub struct ReadFile { pub scope: Option<Arc<WorkspaceScope>> }
///
/// #[tool(description = "Reads a file from the local filesystem", kind = "file")]
/// impl ReadFile {
///     async fn call(
///         &self,
///         #[param(desc = "Absolute path to the file")] path: String,
///         #[param(desc = "Starting line number")] offset: Option<i64>,
///     ) -> rust_agent_core::Result<ToolResult> { ... }
/// }
/// ```
///
/// 从 `call` 方法签名自动生成参数 schema，保留 typed 参数。
///
/// # 用于结构体（兼容旧写法，不推荐）
///
/// ```ignore
/// #[tool(description = "Reads a file...", kind = "file")]
/// pub struct ReadFile { pub scope: Option<Arc<WorkspaceScope>> }
/// // 需在 impl ReadFile 中手动提供 call(&self, args: Value) -> Result<ToolResult>
/// // parameters() 返回空 schema
/// ```
///
/// 可通过 `kind = "..."` 指定分类（`"web"`/`"file"`/`"shell"`/`"skills"`/`"function"`...），默认 `"function"`。
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let (description, kind) = parse_attr(attr);

    // 1. Standalone async fn (external crates like websearch-ai)
    if let Ok(func) = syn::parse::<syn::ItemFn>(item.clone()) {
        return expand_tool_fn(&description, &kind, func);
    }

    // 2. Impl block with typed call method (recommended pattern for stateful tools)
    if let Ok(impl_block) = syn::parse::<syn::ItemImpl>(item.clone()) {
        return expand_tool_impl(&description, &kind, impl_block);
    }

    // 3. Struct (backward-compatible, parameters() returns empty schema)
    if let Ok(input) = syn::parse::<DeriveInput>(item.clone()) {
        return expand_tool_struct(&description, &kind, input);
    }

    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[tool] can only be applied to async functions, impl blocks, or structs",
    )
    .to_compile_error()
    .into()
}

/// Parse `description = "..."` and `kind = "..."` from the attribute token stream.
fn parse_attr(attr: TokenStream) -> (String, String) {
    let mut description = String::new();
    let mut kind = String::new();

    let attr_str = attr.to_string().trim().to_string();
    if attr_str.is_empty() {
        return (description, "function".to_string());
    }

    // Strip outer parens if present
    let inner = if attr_str.starts_with('(') && attr_str.ends_with(')') {
        &attr_str[1..attr_str.len() - 1]
    } else {
        &attr_str
    };

    let inner_ts: proc_macro2::TokenStream = inner.parse().unwrap_or_default();

    // Parse as Punctuated<MetaNameValue, Token![,]>
    if let Ok(pairs) = syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated
        .parse2(inner_ts)
    {
        for nv in pairs {
            if nv.path.is_ident("description") || nv.path.is_ident("desc") {
                if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = nv.value {
                    description = s.value();
                }
            } else if nv.path.is_ident("kind") {
                if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = nv.value {
                    kind = s.value();
                }
            }
        }
    }

    if kind.is_empty() {
        kind = "function".to_string();
    }

    (description, kind)
}

fn expand_tool_fn(description: &str, kind: &str, func: syn::ItemFn) -> TokenStream {
    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();
    let struct_name_str = to_pascal_case(&func_name_str);
    let struct_name = syn::Ident::new(&struct_name_str, func_name.span());
    let args_struct_name = syn::Ident::new(
        &format!("{}Args", struct_name_str),
        func_name.span(),
    );

    // Extract parameters
    let mut param_idents: Vec<syn::Ident> = Vec::new();
    let mut param_types: Vec<syn::Type> = Vec::new();
    let mut param_descs: Vec<String> = Vec::new();
    let mut param_required: Vec<bool> = Vec::new();

    for input in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let name = pat_ident.ident.clone();
                let ty = *pat_type.ty.clone();
                let desc = extract_param_desc(&pat_type.attrs);
                let is_option = is_option_type(&ty);

                param_idents.push(name);
                param_types.push(ty);
                param_descs.push(desc);
                param_required.push(!is_option);
            }
        }
    }

    // Build schema properties
    let schema_props: Vec<_> = param_idents
        .iter()
        .zip(param_types.iter())
        .zip(param_descs.iter())
        .map(|((ident, ty), desc)| {
            let name_str = ident.to_string();
            let type_schema = rust_type_to_schema_tokens(ty);
            if desc.is_empty() {
                quote! {
                    props.insert(#name_str.to_string(), #type_schema);
                }
            } else {
                let desc_str = desc.as_str();
                quote! {
                    {
                        let mut s = #type_schema;
                        s.as_object_mut().unwrap().insert(
                            "description".to_string(),
                            serde_json::Value::String(#desc_str.to_string()),
                        );
                        props.insert(#name_str.to_string(), s);
                    }
                }
            }
        })
        .collect();

    // Required fields
    let required_fields: Vec<_> = param_idents
        .iter()
        .zip(param_required.iter())
        .filter(|(_, req)| **req)
        .map(|(name, _)| name.to_string())
        .collect();

    // Arg struct fields
    let arg_fields = param_idents
        .iter()
        .zip(param_types.iter())
        .map(|(name, ty)| {
            quote! { pub #name: #ty }
        });

    // Build the call method signature
    let call_params = param_idents.iter().zip(param_types.iter()).map(|(name, ty)| {
        quote! { #name: #ty }
    });

    let func_body = &func.block;
    let return_type = &func.sig.output;
    let arg_names = &param_idents;

    let expanded = quote! {
        #[derive(::serde::Deserialize)]
        #[allow(non_snake_case)]
        #[doc(hidden)]
        struct #args_struct_name {
            #(#arg_fields),*
        }

        pub struct #struct_name;

        impl #struct_name {
            pub async fn call(&self, #(#call_params),*) #return_type #func_body
        }

        #[::async_trait::async_trait]
        impl rust_agent_core::ITool for #struct_name {
            fn name(&self) -> &str {
                #func_name_str
            }

            fn description(&self) -> &str {
                #description
            }

            fn parameters(&self) -> serde_json::Value {
                let mut props = serde_json::Map::new();
                #(#schema_props)*
                let mut schema = serde_json::json!({
                    "type": "object",
                    "properties": props,
                });
                let required: Vec<&str> = vec![#(#required_fields),*];
                if !required.is_empty() {
                    schema["required"] = serde_json::Value::Array(
                        required.into_iter().map(|r| serde_json::Value::String(r.to_string())).collect()
                    );
                }
                schema
            }

            async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<rust_agent_core::ToolResult> {
                let args: #args_struct_name = ::serde_json::from_value(arguments)
                    .map_err(|e| rust_agent_core::AgentError::ToolError(
                        format!("Argument deserialization failed: {}", e)
                    ))?;
                Ok(self.call(#(args.#arg_names),*).await)
            }

            fn kind(&self) -> rust_agent_core::ToolKind {
                rust_agent_core::ToolKind::from_macro_literal(#kind)
            }
        }
    };

    expanded.into()
}

fn expand_tool_struct(description: &str, kind: &str, input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let tool_name = struct_name_to_tool_name(&name.to_string());
    let tool_name_lit = proc_macro2::Literal::string(&tool_name);

    let expanded = quote! {
        #input

        #[::async_trait::async_trait]
        impl rust_agent_core::ITool for #name {
            fn name(&self) -> &str {
                #tool_name_lit
            }

            fn description(&self) -> &str {
                #description
            }

            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({"type": "object", "properties": {}})
            }

            async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<rust_agent_core::ToolResult> {
                self.call(arguments).await
            }

            fn kind(&self) -> rust_agent_core::ToolKind {
                rust_agent_core::ToolKind::from_macro_literal(#kind)
            }
        }
    };

    expanded.into()
}

/// Expand `#[tool]` on an inherent impl block containing a `call` method
/// with typed parameters and `#[param(desc)]` annotations.
fn expand_tool_impl(description: &str, kind: &str, item_impl: syn::ItemImpl) -> TokenStream {
    // Validate: must be inherent impl (no trait)
    if item_impl.trait_.is_some() {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[tool] requires an inherent impl block, not a trait impl",
        )
        .to_compile_error()
        .into();
    }

    // Validate: no generics
    if !item_impl.generics.params.is_empty() {
        return syn::Error::new_spanned(
            item_impl.generics,
            "#[tool] does not support generic impl blocks",
        )
        .to_compile_error()
        .into();
    }

    // Extract struct name from self_ty (e.g., `impl EditFile` → `EditFile`)
    let struct_name = match &*item_impl.self_ty {
        syn::Type::Path(type_path) if type_path.qself.is_none() => {
            type_path.path.get_ident().cloned()
        }
        _ => None,
    };
    let struct_name = match struct_name {
        Some(name) => name,
        None => {
            return syn::Error::new_spanned(
                &item_impl.self_ty,
                "#[tool] requires a simple type name in impl block (e.g., `impl MyTool`)",
            )
            .to_compile_error()
            .into();
        }
    };

    let tool_name = struct_name_to_tool_name(&struct_name.to_string());
    let tool_name_lit = proc_macro2::Literal::string(&tool_name);
    let args_struct_name = syn::Ident::new(
        &format!("{}CallArgs", struct_name),
        struct_name.span(),
    );

    // Find the `call` method
    let call_method = item_impl.items.iter().find_map(|item| {
        if let syn::ImplItem::Fn(method) = item {
            if method.sig.ident == "call" {
                return Some(method);
            }
        }
        None
    });

    let call_method = match call_method {
        Some(m) => m,
        None => {
            return syn::Error::new_spanned(
                &item_impl,
                "#[tool] impl block must contain `async fn call(&self, ...)` method",
            )
            .to_compile_error()
            .into();
        }
    };

    // Extract parameters, skipping the receiver (first param)
    let mut param_idents: Vec<syn::Ident> = Vec::new();
    let mut param_types: Vec<syn::Type> = Vec::new();
    let mut param_descs: Vec<String> = Vec::new();
    let mut param_required: Vec<bool> = Vec::new();

    let mut is_receiver = true;
    for input in &call_method.sig.inputs {
        if is_receiver {
            is_receiver = false;
            continue;
        }
        if let syn::FnArg::Typed(pat_type) = input {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let name = pat_ident.ident.clone();
                let ty = *pat_type.ty.clone();
                let desc = extract_param_desc(&pat_type.attrs);
                let is_option = is_option_type(&ty);

                param_idents.push(name);
                param_types.push(ty);
                param_descs.push(desc);
                param_required.push(!is_option);
            }
        }
    }

    // Build schema properties (same logic as expand_tool_fn)
    let schema_props: Vec<_> = param_idents
        .iter()
        .zip(param_types.iter())
        .zip(param_descs.iter())
        .map(|((ident, ty), desc)| {
            let name_str = ident.to_string();
            let type_schema = rust_type_to_schema_tokens(ty);
            if desc.is_empty() {
                quote! {
                    props.insert(#name_str.to_string(), #type_schema);
                }
            } else {
                let desc_str = desc.as_str();
                quote! {
                    {
                        let mut s = #type_schema;
                        s.as_object_mut().unwrap().insert(
                            "description".to_string(),
                            serde_json::Value::String(#desc_str.to_string()),
                        );
                        props.insert(#name_str.to_string(), s);
                    }
                }
            }
        })
        .collect();

    // Required fields
    let required_fields: Vec<_> = param_idents
        .iter()
        .zip(param_required.iter())
        .filter(|(_, req)| **req)
        .map(|(name, _)| name.to_string())
        .collect();

    // Arg struct fields
    let arg_fields = param_idents.iter().zip(param_types.iter()).map(|(name, ty)| {
        quote! { pub #name: #ty }
    });

    let arg_names = &param_idents;

    // Preserve the original impl block (strip #[tool] attribute)
    let impl_attrs: Vec<&syn::Attribute> = item_impl
        .attrs
        .iter()
        .filter(|a| !a.path().is_ident("tool"))
        .collect();

    // Strip #[param] attributes from the re-emitted call method
    let impl_items: Vec<syn::ImplItem> = item_impl
        .items
        .iter()
        .map(|item| match item {
            syn::ImplItem::Fn(method) if method.sig.ident == "call" => {
                let mut method = method.clone();
                for input in &mut method.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = input {
                        pat_type.attrs.retain(|a| !a.path().is_ident("param"));
                    }
                }
                syn::ImplItem::Fn(method)
            }
            _ => item.clone(),
        })
        .collect();

    let expanded = quote! {
        #[derive(::serde::Deserialize)]
        #[allow(non_snake_case)]
        #[doc(hidden)]
        struct #args_struct_name {
            #(#arg_fields),*
        }

        #[::async_trait::async_trait]
        impl rust_agent_core::ITool for #struct_name {
            fn name(&self) -> &str {
                #tool_name_lit
            }

            fn description(&self) -> &str {
                #description
            }

            fn parameters(&self) -> serde_json::Value {
                let mut props = serde_json::Map::new();
                #(#schema_props)*
                let mut schema = serde_json::json!({
                    "type": "object",
                    "properties": props,
                });
                let required: Vec<&str> = vec![#(#required_fields),*];
                if !required.is_empty() {
                    schema["required"] = serde_json::Value::Array(
                        required.into_iter().map(|r| serde_json::Value::String(r.to_string())).collect()
                    );
                }
                schema
            }

            async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<rust_agent_core::ToolResult> {
                let args: #args_struct_name = ::serde_json::from_value(arguments)
                    .map_err(|e| rust_agent_core::AgentError::ToolError(
                        format!("Argument deserialization failed: {}", e)
                    ))?;
                self.call(#(args.#arg_names),*).await
            }

            fn kind(&self) -> rust_agent_core::ToolKind {
                rust_agent_core::ToolKind::from_macro_literal(#kind)
            }
        }

        #(#impl_attrs)*
        impl #struct_name {
            #(#impl_items)*
        }
    };

    expanded.into()
}

/// Extract `#[param(desc = "...")]` or `#[param(description = "...")]` from param attrs.
fn extract_param_desc(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("param") {
            if let Ok(nv) = attr.parse_args::<syn::MetaNameValue>() {
                if nv.path.is_ident("desc") || nv.path.is_ident("description") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        return s.value();
                    }
                }
            }
        }
    }
    String::new()
}

fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .map(|s| s.ident == "Option")
            .unwrap_or(false)
    } else {
        false
    }
}

/// Generate `serde_json::json!({...})` tokens for a Rust type.
fn rust_type_to_schema_tokens(ty: &syn::Type) -> proc_macro2::TokenStream {
    if let syn::Type::Path(type_path) = ty {
        let type_str = quote!(#type_path).to_string().replace(' ', "");

        // Option<T>
        if type_str.starts_with("Option<") {
            let inner = extract_inner_type(&type_str);
            return rust_type_str_to_tokens(&inner);
        }

        // Vec<T>
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

        return rust_type_str_to_tokens(&type_str);
    }

    quote! { serde_json::json!({"type": "string"}) }
}

fn rust_type_str_to_tokens(type_str: &str) -> proc_macro2::TokenStream {
    match type_str {
        "String" | "&str" | "str" => quote! { serde_json::json!({"type": "string"}) },
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
        | "u128" | "usize" => quote! { serde_json::json!({"type": "integer"}) },
        "f32" | "f64" => quote! { serde_json::json!({"type": "number"}) },
        "bool" => quote! { serde_json::json!({"type": "boolean"}) },
        _ => quote! { serde_json::json!({"type": "string"}) },
    }
}

fn extract_inner_type(type_str: &str) -> String {
    let start = type_str.find('<').map(|i| i + 1).unwrap_or(0);
    let end = type_str.rfind('>').unwrap_or(type_str.len());
    type_str[start..end].to_string()
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Convert CamelCase struct name to the exposed tool name.
///
/// Examples:
/// - `ReadFile` → `"read_file"`
/// - `LoadSkillTool` → `"load_skill"`  (strips trailing `_tool`)
/// - `RunCommand` → `"run_command"`
fn struct_name_to_tool_name(name: &str) -> String {
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_uppercase() {
            if !result.is_empty() {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    // Strip trailing _tool suffix: structs named `XxxTool` expose tool name `xxx`
    if result.ends_with("_tool") {
        result.truncate(result.len() - 5);
    }
    result
}
