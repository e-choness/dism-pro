//! Target actors (doc 02 §3): one thread per target owns that target's context (DISM
//! session, journal writer, …) and runs requests one at a time, in order. Different targets
//! run in parallel.
//!
//! The context is created *on* the actor thread, so it may be `!Send` (DISM sessions are).

use std::{
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

type Request<C> = Box<dyn FnOnce(&mut C) + Send>;

/// The actor thread has stopped (its context failed to start, or a request panicked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("target actor is not running")]
pub struct ActorGone;

/// Handle to a running actor. Dropping it lets queued requests finish, then joins the thread.
#[derive(Debug)]
pub struct TargetActor<C> {
    tx: Option<Sender<Request<C>>>,
    thread: Option<JoinHandle<()>>,
}

impl<C: 'static> TargetActor<C> {
    /// Starts an actor thread named `name` whose context is built by `init` on that thread.
    /// If `init` fails, every later request returns [`ActorGone`].
    ///
    /// # Errors
    /// The OS refused to create the thread.
    pub fn spawn<E: Send + 'static>(
        name: &str,
        init: impl FnOnce() -> Result<C, E> + Send + 'static,
    ) -> io::Result<(Self, Receiver<Result<(), E>>)> {
        let (tx, rx) = mpsc::channel::<Request<C>>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut context = match init() {
                    Ok(c) => {
                        let _ = ready_tx.send(Ok(())); // ignore: the caller may not wait for readiness
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e)); // ignore: as above
                        return;
                    }
                };
                for request in rx {
                    request(&mut context);
                }
            })?;
        Ok((
            Self {
                tx: Some(tx),
                thread: Some(thread),
            },
            ready_rx,
        ))
    }

    /// Queues `f` and returns a receiver for its result.
    ///
    /// # Errors
    /// [`ActorGone`] if the actor has stopped.
    pub fn submit<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut C) -> R + Send + 'static,
    ) -> Result<Receiver<R>, ActorGone> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let request: Request<C> = Box::new(move |c| {
            let _ = reply_tx.send(f(c)); // ignore: the caller may have stopped waiting
        });
        self.tx
            .as_ref()
            .ok_or(ActorGone)?
            .send(request)
            .map_err(|_| ActorGone)?;
        Ok(reply_rx)
    }

    /// Runs `f` on the actor and waits for its result.
    ///
    /// # Errors
    /// [`ActorGone`] if the actor stopped before answering.
    pub fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut C) -> R + Send + 'static,
    ) -> Result<R, ActorGone> {
        self.submit(f)?.recv().map_err(|_| ActorGone)
    }
}

impl<C> Drop for TargetActor<C> {
    fn drop(&mut self) {
        drop(self.tx.take()); // closes the queue; the thread exits after the last request
        if let Some(t) = self.thread.take() {
            let _ = t.join(); // ignore: a panicked actor has nothing left to report
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn runs_requests_in_order_on_one_thread() {
        let (actor, ready) = TargetActor::spawn("t", || Ok::<_, ()>(Vec::<u32>::new())).unwrap();
        assert_eq!(ready.recv().unwrap(), Ok(()));
        let pending: Vec<_> = (0..100)
            .map(|i| actor.submit(move |v: &mut Vec<u32>| v.push(i)).unwrap())
            .collect();
        for p in pending {
            p.recv().unwrap();
        }
        let seen = actor.call(|v| v.clone()).unwrap();
        assert_eq!(seen, (0..100).collect::<Vec<_>>());
        let name = actor
            .call(|_| thread::current().name().map(str::to_owned))
            .unwrap();
        assert_eq!(name.as_deref(), Some("t"));
    }

    #[test]
    fn context_may_be_not_send() {
        // Rc is !Send: it can only exist because it's created on the actor thread.
        let (actor, _) =
            TargetActor::spawn("rc", || Ok::<_, ()>(Rc::new(RefCell::new(0)))).unwrap();
        actor.call(|c| *c.borrow_mut() += 1).unwrap();
        assert_eq!(actor.call(|c| *c.borrow()).unwrap(), 1);
    }

    #[test]
    fn failed_init_reports_and_rejects_requests() {
        let (actor, ready) = TargetActor::<u8>::spawn("bad", || Err("no session")).unwrap();
        assert_eq!(ready.recv().unwrap(), Err("no session"));
        assert_eq!(actor.call(|c| *c), Err(ActorGone));
    }
}
