use std::sync::Arc;

pub mod worker_pool;
// generally speaking it can be used in hypothetical async production env
// just tokio is dev dep for now so it has to be disable unless test
#[cfg(test)]
pub mod tokio;

pub trait Submit<State> {
    fn stateful_boxed(&self, task: Box<dyn FnOnce(&mut State) + Send>);
    fn stateless_boxed(&self, task: Box<dyn FnOnce() + Send>);
}

impl<T, S> Submit<S> for Arc<T>
where
    T: Submit<S>,
{
    fn stateful_boxed(&self, task: Box<dyn FnOnce(&mut S) + Send>) {
        T::stateful_boxed(self, task)
    }
    fn stateless_boxed(&self, task: Box<dyn FnOnce() + Send>) {
        T::stateless_boxed(self, task)
    }
}

pub trait SubmitExt<State> {
    fn stateful(&self, task: impl FnOnce(&mut State) + Send + 'static);
    fn stateless(&self, task: impl FnOnce() + Send + 'static);
}
impl<T: Submit<S>, S> SubmitExt<S> for T {
    fn stateful(&self, task: impl FnOnce(&mut S) + Send + 'static) {
        self.stateful_boxed(Box::new(task))
    }
    fn stateless(&self, task: impl FnOnce() + Send + 'static) {
        self.stateless_boxed(Box::new(task))
    }
}
