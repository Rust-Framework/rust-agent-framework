use rust_agent_core::ChatMessage;

/// Prompt template for constructing structured prompts.
///
/// Following MAF's prompt engineering patterns, supports
/// variable interpolation and system/user message assembly.
pub struct PromptTemplate {
    system_template: Option<String>,
    user_template: String,
}

impl PromptTemplate {
    pub fn new(user_template: impl Into<String>) -> Self {
        Self {
            system_template: None,
            user_template: user_template.into(),
        }
    }

    pub fn with_system(mut self, system_template: impl Into<String>) -> Self {
        self.system_template = Some(system_template.into());
        self
    }

    /// Render the template with variable substitution.
    ///
    /// Variables use `{{key}}` syntax.
    pub fn render(&self, vars: &std::collections::HashMap<&str, &str>) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        if let Some(sys) = &self.system_template {
            messages.push(ChatMessage::system(Self::substitute(sys, vars)));
        }

        messages.push(ChatMessage::user(Self::substitute(&self.user_template, vars)));
        messages
    }

    fn substitute(template: &str, vars: &std::collections::HashMap<&str, &str>) -> String {
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }
}
