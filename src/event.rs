pub struct Dispatcher<E> {
	listeners: Vec<Box<dyn Fn(&E)>>
}

impl<E> Dispatcher<E> {
	pub fn new() -> Dispatcher<E> {
		Dispatcher {
			listeners: Vec::new()
		}
	}

	pub fn add_listener<F: 'static + Fn(&E)>(&mut self, listener: F) {
		self.listeners.push(Box::new(listener));
	}

	pub fn dispatch(&self, event: E) {
		for listener in &self.listeners {
			listener(&event);
		}
	}
}
