use dashmap::DashMap;
use tokio::sync::mpsc;

pub struct SessionManager {
    sessions: DashMap<String, mpsc::UnboundedSender<String>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn create_session(&self) -> (String, mpsc::UnboundedReceiver<String>) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        self.sessions.insert(session_id.clone(), tx);
        (session_id, rx)
    }

    pub fn send(&self, session_id: &str, message: String) -> Result<(), &'static str> {
        if let Some(tx) = self.sessions.get(session_id) {
            if tx.send(message).is_err() {
                return Err("Failed to send message over channel");
            }
            Ok(())
        } else {
            Err("Session not found")
        }
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.sessions.contains_key(session_id)
    }

    pub fn remove_session(&self, session_id: &str) {
        self.sessions.remove(session_id);
    }
}
