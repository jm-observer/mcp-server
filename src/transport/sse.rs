use dashmap::DashMap;
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time;

#[derive(Clone)]
pub struct SessionEntry {
    tx: mpsc::UnboundedSender<String>,
    last_seen_at: Instant,
    sse_connected: bool,
}

pub struct SessionManager {
    sessions: Arc<DashMap<String, SessionEntry>>,
    ttl: Duration,
    cleanup_interval: Duration,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        let manager = Self {
            sessions: Arc::new(DashMap::new()),
            ttl: Duration::from_secs(90),
            cleanup_interval: Duration::from_secs(15),
        };
        let sessions = manager.sessions.clone();
        let ttl = manager.ttl;
        let interval = manager.cleanup_interval;
        tokio::spawn(async move {
            loop {
                time::sleep(interval).await;
                let now = Instant::now();
                let expired_keys: Vec<String> = sessions
                    .iter()
                    .filter(|e| {
                        let s = e.value();
                        !s.sse_connected && now.duration_since(s.last_seen_at) > ttl
                    })
                    .map(|e| e.key().clone())
                    .collect();
                for k in &expired_keys {
                    sessions.remove(k);
                    info!("Session {} expired and removed (ttl={}s)", k, ttl.as_secs());
                }
                if !expired_keys.is_empty() {
                    debug!("Cleanup: removed {} expired sessions", expired_keys.len());
                }
            }
        });
        manager
    }

    pub fn create_session(&self) -> (String, mpsc::UnboundedReceiver<String>) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let entry = SessionEntry {
            tx,
            last_seen_at: Instant::now(),
            sse_connected: true,
        };
        self.sessions.insert(session_id.clone(), entry);
        (session_id, rx)
    }

    pub fn send(&self, session_id: &str, message: String) -> Result<(), &'static str> {
        if let Some(entry) = self.sessions.get(session_id) {
            if entry.tx.send(message).is_err() {
                drop(entry);
                self.mark_disconnected(session_id);
                warn!("Send failed for session {}, marked as disconnected", session_id);
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

    /// Update last_seen_at to now for given session
    pub fn touch(&self, session_id: &str) {
        if let Some(mut entry) = self.sessions.get_mut(session_id) {
            entry.last_seen_at = Instant::now();
        }
    }

    /// Mark a session's SSE connection as disconnected
    pub fn mark_disconnected(&self, session_id: &str) {
        if let Some(mut entry) = self.sessions.get_mut(session_id) {
            entry.sse_connected = false;
        }
    }

}
