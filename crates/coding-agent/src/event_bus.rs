use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

type Handler = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Clone, Default)]
pub struct EventBus {
    handlers: Arc<Mutex<BTreeMap<String, BTreeMap<u64, Handler>>>>,
    next_id: Arc<Mutex<u64>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit(&self, channel: &str, data: Value) {
        let handlers = self
            .handlers
            .lock()
            .expect("event handlers lock should not be poisoned")
            .get(channel)
            .cloned()
            .unwrap_or_default();
        for handler in handlers.values() {
            let handler = handler.clone();
            let data = data.clone();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(data)));
        }
    }

    pub fn on(
        &self,
        channel: impl Into<String>,
        handler: impl Fn(Value) + Send + Sync + 'static,
    ) -> EventSubscription {
        let channel = channel.into();
        let mut next_id = self
            .next_id
            .lock()
            .expect("event id lock should not be poisoned");
        let id = *next_id;
        *next_id += 1;
        self.handlers
            .lock()
            .expect("event handlers lock should not be poisoned")
            .entry(channel.clone())
            .or_default()
            .insert(id, Arc::new(handler));
        EventSubscription {
            bus: self.clone(),
            channel,
            id,
        }
    }

    pub fn clear(&self) {
        self.handlers
            .lock()
            .expect("event handlers lock should not be poisoned")
            .clear();
    }

    fn off(&self, channel: &str, id: u64) {
        if let Some(handlers) = self
            .handlers
            .lock()
            .expect("event handlers lock should not be poisoned")
            .get_mut(channel)
        {
            handlers.remove(&id);
        }
    }
}

pub struct EventSubscription {
    bus: EventBus,
    channel: String,
    id: u64,
}

impl EventSubscription {
    pub fn unsubscribe(self) {}
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        self.bus.off(&self.channel, self.id);
    }
}

pub fn create_event_bus() -> EventBus {
    EventBus::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn event_bus_emits_and_unsubscribes() {
        let bus = create_event_bus();
        let seen = Arc::new(Mutex::new(0));
        let seen_clone = seen.clone();
        let sub = bus.on("test", move |value| {
            *seen_clone.lock().expect("lock should work") = value["n"].as_i64().unwrap_or_default();
        });
        bus.emit("test", json!({"n": 3}));
        assert_eq!(*seen.lock().expect("lock should work"), 3);
        drop(sub);
        bus.emit("test", json!({"n": 4}));
        assert_eq!(*seen.lock().expect("lock should work"), 3);
    }
}
