use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, ExprLit, Lit, Meta};

/// `#[tool]` attribute macro — simplifies ITool definition.
///
/// # On async functions
///
/// ```ignore
/// #[tool(description = "Echoes back the input text")]
/// async fn echo(#[param(desc = "Text to echo")] text: String) -> String {
///     text
/// }
/// ```
///
/// Generates a struct (PascalCase of fn name) implementing `ITool`,
/// with auto-derived JSON schema and argument deserialization.
///
/// # On unit structs
///
/// ```ignore
/// #[tool(description = "My tool")]
/// struct MyTool;
/// ```
///
/// Generates `ITool` impl delegating to a `call` method you define.
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let description = parse_description(attr);

    // Try as function
    if let Ok(func) = syn::parse::<syn::ItemFn>(item.clone()) {
        return expand_tool_fn(&description, func);
    }

    // Try as struct
    if let Ok(input) = syn::parse::<DeriveInput>(item.clone()) {
        return expand_tool_struct(&description, input);
    }

    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[tool] can only be applied to async functions or unit structs",
    )
    .to_compile_error()
    .into()
}

/// Parse `description = "..."` from the attribute token stream.
fn parse_description(attr: TokenStream) -> String {
    if attr.is_empty() {
        return String::new();
    }
    if let Ok(meta) = syn::parse::<Meta>(attr) {
        if let Meta::NameValue(nv) = meta {
            if nv.path.is_ident("description") || nv.path.is_ident("desc") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = nv.value
                {
                    return s.value();
                }
            }
        }
    }
    String::new()
}

fn expand_tool_fn(description: &str, func: syn::ItemFn) -> TokenStream {
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

    // Build the call method signature: async fn call(&self, text: String) -> String
    let call_params = param_idents.iter().zip(param_types.iter()).map(|(name, ty)| {
        quote! { #name: #ty }
    });

    let func_body = &func.block;
    let return_type = &func.sig.output;
    let arg_names = &param_idents;

    let expanded = quote! {
        /// Auto-generated args struct by #[tool] macro.
        #[derive(::serde::Deserialize)]
        #[allow(non_snake_case)]
        struct #args_struct_name {
            #(#arg_fields),*
        }

        /// Auto-generated tool struct by #[tool] macro.
        pub struct #struct_name;

        impl #struct_name {
            /// The original function logic.
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

            fn parameters_schema(&self) -> serde_json::Value {
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

            async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<String> {
                let args: #args_struct_name = ::serde_json::from_value(arguments)
                    .map_err(|e| rust_agent_core::AgentError::ToolError(
                        format!("Argument deserialization failed: {}", e)
                    ))?;
                let result = self.call(#(args.#arg_names),*).await;
                Ok(result)
            }
        }
    };

    expanded.into()
}

fn expand_tool_struct(description: &str, input: DeriveInput) -> TokenStream {
    let name = &input.ident;

    let expanded = quote! {
        #input

        #[::async_trait::async_trait]
        impl rust_agent_core::ITool for #name {
            fn name(&self) -> &str {
                stringify!(#name)
            }

            fn description(&self) -> &str {
                #description
            }

            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({"type": "object", "properties": {}})
            }

            async fn execute(&self, _arguments: serde_json::Value) -> rust_agent_core::Result<String> {
                self.call(_arguments).await
            }
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
