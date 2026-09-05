//! Player notification hub. Every connected client holds one WebSocket;
//! the server pushes lifecycle events the moment they happen so the queue
//! never feels like a black box:
//!
//! - `queue_status`  — depth, your wait, current skill window (throttled)
//! - `match_found`   — opponent card + connect info
//! - `match_result`  — outcome, rating deltas, attestation when signed
//! - `match_update`  — state transitions (disputed, cancelled, settling…)
//!
//! A wallet may hold several connections (two tabs); all of them get every
//! event. Sends are best-effort: a dead socket is dropped, never blocks the
//! matchmaker.

use dashmap::DashMap;
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct WsHub {
    senders: DashMap<String, Vec<mpsc::UnboundedSender<String>>>,
}

impl WsHub {
    pub fn new() -> Self {
        Self {
            senders: DashMap::new(),
        }
    }

    pub fn register(&self, wallet: &str) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.senders.entry(wallet.to_string()).or_default().push(tx);
        // Prune closed channels lazily on every register.
        if let Some(mut v) = self.senders.get_mut(wallet) {
            v.retain(|tx| !tx.is_closed());
        }
        rx
    }

    pub fn send(&self, wallet: &str, event_type: &str, payload: Value) {
        let msg = json!({ "type": event_type, "data": payload }).to_string();
        if let Some(mut conns) = self.senders.get_mut(wallet) {
            conns.retain(|tx| tx.send(msg.clone()).is_ok());
        }
    }

    #[allow(dead_code)] // reserved for global announcements (maintenance, seasons)
    pub fn broadcast(&self, event_type: &str, payload: Value) {
        let msg = json!({ "type": event_type, "data": payload }).to_string();
        for mut entry in self.senders.iter_mut() {
            entry.value_mut().retain(|tx| tx.send(msg.clone()).is_ok());
        }
    }

    pub fn connected_count(&self) -> usize {
        self.senders.len()
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn events_reach_all_sockets_for_wallet() {
        let hub = WsHub::new();
        let mut rx1 = hub.register("0xa");
        let mut rx2 = hub.register("0xa");
        hub.send("0xa", "match_found", json!({ "matchId": "m1" }));
        assert_eq!(
            rx1.recv().await.unwrap(),
            r#"{"data":{"matchId":"m1"},"type":"match_found"}"#
        );
        assert_eq!(
            rx2.recv().await.unwrap(),
            r#"{"data":{"matchId":"m1"},"type":"match_found"}"#
        );
    }

    #[tokio::test]
    async fn other_wallets_do_not_receive() {
        let hub = WsHub::new();
        let mut rx_a = hub.register("0xa");
        let mut rx_b = hub.register("0xb");
        hub.send("0xa", "queue_status", json!({ "depth": 2 }));
        assert!(rx_a.recv().await.is_some());
        assert!(rx_b.try_recv().is_err());
    }

    #[tokio::test]
    async fn closed_sockets_are_pruned() {
        let hub = WsHub::new();
        let rx1 = hub.register("0xa");
        let mut rx2 = hub.register("0xa");
        drop(rx1);
        hub.send("0xa", "ping", json!({}));
        assert!(rx2.recv().await.is_some());
        assert_eq!(hub.senders.get("0xa").unwrap().len(), 1);
    }
}
