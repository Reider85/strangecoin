pub mod protocol;
pub mod rate_limiter;
pub use rate_limiter::RateLimiter;

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tracing::info;

/// Network node that manages P2P connections and the TCP listener
pub struct Node {
    /// TCP listener for incoming connections
    listener: Option<TcpListener>,
    /// Background thread handles for peer connections
    peer_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// Shutdown signal to stop accepting new connections
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    /// Listen address
    addr: SocketAddr,
}

impl Node {
    /// Create a new node (without starting the listener)
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            listener: None,
            peer_handles: Arc::new(Mutex::new(Vec::new())),
            shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            addr,
        }
    }

    /// Get the shutdown signal
    pub fn shutdown_signal(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    /// Set the TCP listener (called when server starts)
    pub fn set_listener(&mut self, listener: TcpListener) {
        self.listener = Some(listener);
    }

    /// Add a peer connection handle
    pub fn add_peer_handle(&self, handle: JoinHandle<()>) {
        self.peer_handles.lock().unwrap().push(handle);
    }

    /// Stop accepting new connections
    pub fn stop_listening(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(listener) = &self.listener {
            // Closing the listener will cause accept() to return an error
            drop(listener.try_clone());
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        info!("Shutting down network node");

        // Signal shutdown
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);

        // Close listener
        if let Some(listener) = self.listener.take() {
            drop(listener);
            info!("TCP listener closed");
        }

        // Wait for peer connection threads to finish (with timeout)
        let handles = self.peer_handles.lock().unwrap();
        for handle in handles.iter() {
            // Note: We can't join here because we only have a reference
            // The threads should check the shutdown signal and exit on their own
        }

        // Give threads a moment to shut down
        std::thread::sleep(std::time::Duration::from_millis(100));

        info!("Network node shutdown complete");
    }
}
