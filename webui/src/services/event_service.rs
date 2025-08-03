use crate::model::EventMessage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

type Subscriber = RefCell<HashMap<usize, Box<dyn Fn(EventMessage)>>>;

pub struct EventService {
    subscriber_id: Rc<AtomicUsize>,
    subscribers: Rc<Subscriber>,
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