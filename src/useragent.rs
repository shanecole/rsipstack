
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserAgent {
    user_agent: String,    
}

pub struct UserAgentBuilder {
    user_agent: String,
}

impl UserAgentBuilder {
    pub fn new() -> Self {
        UserAgentBuilder {
            user_agent: "RustSIP".to_string(),
        }
    }

    pub fn user_agent(&mut self, user_agent: &str) -> &mut Self {
        self.user_agent = user_agent.to_string();
        self
    }

    pub fn build(&self) -> UserAgent {
        UserAgent {
            user_agent: self.user_agent.clone(),
        }
    }
}

impl UserAgent {
    pub fn builder() -> UserAgentBuilder {
        UserAgentBuilder::new()
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub async fn start(&self) {
        log::info!("Starting UserAgent: {}", self.user_agent);
    }

    pub async fn shutdown(&self) {
        log::info!("Shutting down UserAgent: {}", self.user_agent);
    }
}