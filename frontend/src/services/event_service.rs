use crate::model::EventMessage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type Subscriber = RefCell<HashMap<usize, Box<dyn Fn(EventMessage)>>>;

pub struct EventService {
    subscriber_id: Rc<AtomicUsize>,
    subscribers: Rc<Subscriber>,
    block_config_updated_message: Rc<AtomicBool>,
}

impl Default for EventService {
    fn default() -> Self {
        Self::new()
    }
}


impl EventService {
    pub fn new() -> Self {
        Self {
            subscriber_id: Rc::new(AtomicUsize::new(0)),
            subscribers: Rc::new(RefCell::new(HashMap::new())),
            block_config_updated_message: Rc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_config_change_message_blocked(&self) -> bool {
        self.block_config_updated_message.load(Ordering::Relaxed)
    }

    pub fn set_config_change_message_blocked(&self, value: bool)  {
        if value {
            self.block_config_updated_message.store(value, Ordering::Relaxed);
        } else {
            let flag = Rc::clone(&self.block_config_updated_message);
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(500).await;
                flag.store(false, Ordering::Relaxed);
            });
        }
    }

    pub fn subscribe<F: Fn(EventMessage) + 'static>(&self, callback: F) -> usize {
        let sub_id = self.subscriber_id.fetch_add(1, Ordering::SeqCst);
        self.subscribers.borrow_mut().insert(sub_id, Box::new(callback));
        sub_id
    }

    pub fn unsubscribe(&self, sub_id: usize) {
        self.subscribers.borrow_mut().remove(&sub_id);
    }

    pub fn broadcast(&self, msg: EventMessage) {
        for (_, cb) in self.subscribers.borrow().iter() {
            cb(msg.clone());
        }
    }
}